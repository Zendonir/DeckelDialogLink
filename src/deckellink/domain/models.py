"""Configuration and domain models.

All machine-specific values for the DIALOG 11 are *assumptions* unless a
profile is explicitly marked ``verified``. See ``docs/VERIFICATION_CHECKLIST.md``.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import StrEnum
from typing import Any


class Parity(StrEnum):
    NONE = "none"
    EVEN = "even"
    ODD = "odd"
    MARK = "mark"
    SPACE = "space"

    @property
    def letter(self) -> str:
        return {"none": "N", "even": "E", "odd": "O", "mark": "M", "space": "S"}[self.value]

    @property
    def label(self) -> str:
        return {
            "none": "Keine",
            "even": "Gerade (Even)",
            "odd": "Ungerade (Odd)",
            "mark": "Mark",
            "space": "Space",
        }[self.value]


class LineEnding(StrEnum):
    CRLF = "crlf"
    LF = "lf"
    CR = "cr"

    @property
    def sequence(self) -> bytes:
        return {"crlf": b"\r\n", "lf": b"\n", "cr": b"\r"}[self.value]

    @property
    def label(self) -> str:
        return {"crlf": "CRLF (0D 0A)", "lf": "LF (0A)", "cr": "CR (0D)"}[self.value]


class VerificationStatus(StrEnum):
    UNVERIFIED = "unverified"
    VERIFIED = "verified"

    @property
    def label(self) -> str:
        return "an Maschine verifiziert" if self is VerificationStatus.VERIFIED else "nicht verifiziert"


class TabHandling(StrEnum):
    """What to do with TAB characters in NC files."""

    REJECT = "reject"
    REPLACE = "replace"
    KEEP = "keep"


class WhitespaceHandling(StrEnum):
    NONE = "none"
    TRAILING = "trailing"
    BOTH = "both"


class CommentHandling(StrEnum):
    KEEP = "keep"
    REMOVE = "remove"


class EndDetection(StrEnum):
    """How the end of a received program is detected."""

    INACTIVITY = "inactivity"
    END_SEQUENCE = "end_sequence"
    BOTH = "both"

    @property
    def uses_end_sequence(self) -> bool:
        return self in (EndDetection.END_SEQUENCE, EndDetection.BOTH)

    @property
    def uses_inactivity(self) -> bool:
        return self in (EndDetection.INACTIVITY, EndDetection.BOTH)


STANDARD_BAUD_RATES: tuple[int, ...] = (
    50,
    75,
    110,
    134,
    150,
    200,
    300,
    600,
    1200,
    1800,
    2400,
    4800,
    9600,
    19200,
    38400,
    57600,
    115200,
)
DATA_BITS: tuple[int, ...] = (5, 6, 7, 8)
STOP_BITS: tuple[float, ...] = (1, 1.5, 2)


@dataclass(frozen=True)
class MachineInfo:
    manufacturer: str = "DECKEL"
    model: str = "FP2A"
    control: str = "DIALOG 11"


@dataclass(frozen=True)
class SerialSettings:
    """RS-232 parameters. Defaults are conservative assumptions, not verified values."""

    baud_rate: int = 4800
    data_bits: int = 7
    parity: Parity = Parity.NONE
    stop_bits: float = 1
    rts_cts: bool = False
    xon_xoff: bool = False
    dsr_dtr: bool = False
    initial_rts: bool = True
    initial_dtr: bool = False
    require_cts_before_send: bool = False
    require_dsr_before_send: bool = False
    read_timeout_ms: int = 1000
    write_timeout_ms: int = 10000
    inter_byte_timeout_ms: int = 0
    handshake_timeout_ms: int = 30000

    @property
    def frame(self) -> str:
        stop = "1.5" if self.stop_bits == 1.5 else str(int(self.stop_bits))
        return f"{self.data_bits}{self.parity.letter}{stop}"

    @property
    def flow_control_label(self) -> str:
        parts = []
        if self.rts_cts:
            parts.append("RTS/CTS")
        if self.xon_xoff:
            parts.append("XON/XOFF")
        if self.dsr_dtr:
            parts.append("DSR/DTR")
        return " + ".join(parts) if parts else "Kein Handshake"

    @property
    def bits_per_character(self) -> float:
        parity_bits = 0 if self.parity is Parity.NONE else 1
        return 1 + self.data_bits + parity_bits + self.stop_bits

    @property
    def bytes_per_second(self) -> float:
        return self.baud_rate / self.bits_per_character

    @property
    def summary(self) -> str:
        return f"{self.baud_rate} Baud | {self.frame} | {self.flow_control_label}"


@dataclass(frozen=True)
class TransferSettings:
    """Settings for preparing and sending NC data."""

    encoding: str = "ascii"
    line_ending: LineEnding = LineEnding.CRLF
    program_start: str = ""
    program_end: str = "M30"
    append_program_end: bool = False
    start_delay_ms: int = 1000
    end_delay_ms: int = 1000
    character_delay_ms: int = 0
    packet_size: int = 0
    packet_delay_ms: int = 0
    remove_blank_lines: bool = True
    strip_null_bytes: bool = True
    skip_semicolon_lines: bool = False
    normalize_line_endings: bool = True
    tab_handling: TabHandling = TabHandling.REPLACE
    whitespace: WhitespaceHandling = WhitespaceHandling.TRAILING
    comment_handling: CommentHandling = CommentHandling.KEEP
    comment_open: str = "("
    comment_close: str = ")"
    ensure_final_newline: bool = True
    send_eot_trailer: bool = True
    max_line_length: int = 120
    max_program_bytes: int = 500_000
    close_port_after_send: bool = True


@dataclass(frozen=True)
class ReceiveSettings:
    """Settings for receiving programs from the CNC."""

    end_detection: EndDetection = EndDetection.BOTH
    end_sequence: str = "M30"
    inactivity_timeout_ms: int = 5000
    first_byte_timeout_ms: int = 120_000
    end_grace_ms: int = 500
    max_receive_bytes: int = 2_000_000
    strip_null_bytes: bool = True
    normalize_line_endings: bool = True
    save_raw: bool = True
    end_on_eot: bool = True
    program_name_pattern: str = ""


class ProtocolMode(StrEnum):
    """Transfer protocol on top of the serial line."""

    PLAIN = "plain"
    LSV2 = "lsv2"

    @property
    def label(self) -> str:
        return {
            "plain": "Ohne Protokoll (Klartext)",
            "lsv2": "LSV2 (DIN 66019, Telegramme mit Quittung)",
        }[self.value]


class BccMode(StrEnum):
    """How the block check character of an LSV2 telegram is formed (unverified for DIALOG 11)."""

    TEXT_ETX = "text_etx"  # XOR of the (unstuffed) text and ETX
    TRANSMITTED = "transmitted"  # XOR of all transmitted bytes after STX up to and including ETX
    NONE = "none"  # no BCC byte


@dataclass(frozen=True)
class ProtocolSettings:
    """LSV2 / DIN 66019 link settings.

    The link procedure (ENQ, DLE 0 / DLE 1 acknowledgements, NAK, WACK, EOT) follows
    the public DIN 66019 / ISO 1745 basic mode. Every detail of how the DIALOG 11
    applies it is an assumption and therefore configurable - see
    ``docs/VERIFICATION_CHECKLIST.md``.
    """

    mode: ProtocolMode = ProtocolMode.PLAIN
    transparent: bool = True
    bcc_mode: BccMode = BccMode.TEXT_ETX
    enq_interval_ms: int = 2000
    link_timeout_ms: int = 120_000
    ack_timeout_ms: int = 5000
    drip_feed_ack_timeout_ms: int = 600_000
    max_retries: int = 3
    wack_poll_ms: int = 1000
    lines_per_telegram: int = 1
    max_telegram_length: int = 256
    include_line_ending: bool = True
    start_telegrams: tuple[str, ...] = ()
    end_telegrams: tuple[str, ...] = ()
    send_eot_on_cancel: bool = True

    @property
    def is_lsv2(self) -> bool:
        return self.mode is ProtocolMode.LSV2


@dataclass(frozen=True)
class ParameterSettings:
    """Transfer of machine parameter files.

    DeckelLink never changes parameters on its own. Reading requires the operator
    to start the parameter output at the CNC; writing requires the authorised
    operator to enable parameter input at the CNC. The parameter file format of
    the DIALOG 11 is not verified - see ``docs/PARAMETERS.md``.
    """

    allow_write: bool = False
    inactivity_timeout_ms: int = 5000
    first_byte_timeout_ms: int = 120_000
    max_bytes: int = 1_000_000
    identification_pattern: str = ""
    max_backup_age_min: int = 60


@dataclass(frozen=True)
class Profile:
    """A complete communication profile for one machine/control configuration."""

    id: str
    display_name: str
    machine: MachineInfo = field(default_factory=MachineInfo)
    verification_status: VerificationStatus = VerificationStatus.UNVERIFIED
    serial: SerialSettings = field(default_factory=SerialSettings)
    transfer: TransferSettings = field(default_factory=TransferSettings)
    receive: ReceiveSettings = field(default_factory=ReceiveSettings)
    protocol: ProtocolSettings = field(default_factory=ProtocolSettings)
    parameters: ParameterSettings = field(default_factory=ParameterSettings)
    description: str = ""
    schema_version: int = 1
    builtin: bool = field(default=False, compare=False)

    def to_dict(self) -> dict[str, Any]:
        """Serialize to the JSON profile format (``builtin`` is not persisted)."""
        return {
            "schema_version": self.schema_version,
            "id": self.id,
            "display_name": self.display_name,
            "description": self.description,
            "machine": {
                "manufacturer": self.machine.manufacturer,
                "model": self.machine.model,
                "control": self.machine.control,
            },
            "verification_status": self.verification_status.value,
            "serial": _dataclass_to_dict(self.serial),
            "transfer": _dataclass_to_dict(self.transfer),
            "receive": _dataclass_to_dict(self.receive),
            "protocol": _dataclass_to_dict(self.protocol),
            "parameters": _dataclass_to_dict(self.parameters),
        }


def _dataclass_to_dict(obj: Any) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for name in obj.__dataclass_fields__:
        value = getattr(obj, name)
        if isinstance(value, StrEnum):
            value = value.value
        elif isinstance(value, float) and value.is_integer():
            value = int(value)
        elif isinstance(value, tuple):
            value = list(value)
        result[name] = value
    return result


FALLBACK_PROFILE = Profile(
    id="fallback_7n1_rtscts",
    display_name="Interner Notfall-Standard - 4800 7N1 RTS/CTS",
    description=(
        "Fest eingebautes Ersatzprofil, falls keine Profildatei geladen werden konnte. "
        "Werte entsprechen Profil A und sind nicht verifiziert."
    ),
    serial=SerialSettings(
        baud_rate=4800,
        data_bits=7,
        parity=Parity.NONE,
        stop_bits=1,
        rts_cts=True,
        initial_rts=True,
        require_cts_before_send=True,
    ),
    builtin=True,
)
