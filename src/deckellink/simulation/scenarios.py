"""Predefined simulator scenarios."""

from __future__ import annotations

from dataclasses import dataclass, replace
from enum import StrEnum
from typing import Any

from deckellink.domain.models import ProtocolMode
from deckellink.simulation.lsv2_cnc import Lsv2SimConfig
from deckellink.transport.simulated_transport import SimulationConfig

SAMPLE_PROGRAM = (
    b"(DECKELLINK SIMULATOR TESTPROGRAMM)\r\n"
    b"N10 G90 G17\r\n"
    b"N20 G00 X0 Y0 Z50\r\n"
    b"N30 T1 M06\r\n"
    b"N40 S1200 M03\r\n"
    b"N50 G01 Z-1 F100\r\n"
    b"N60 X40 F200\r\n"
    b"N70 Y30\r\n"
    b"N80 X0\r\n"
    b"N90 Y0\r\n"
    b"N100 G00 Z50 M05\r\n"
    b"N110 M30\r\n"
)

# Clearly artificial parameter list for the simulator. This is NOT the DIALOG 11
# parameter format (which is not verified) - it only exercises the data path.
SAMPLE_PARAMETERS = (
    b"(SIMULIERTE MASCHINENPARAMETER - KEIN ECHTES DIALOG-11-FORMAT)\r\n"
    + b"".join(b"P%04d=%d\r\n" % (n, (n * 37) % 1000) for n in range(1, 41))
    + b"(ENDE SIMULATION)\r\n"
)

INVALID_PROGRAM = b"(PROGRAMM MIT UNG\xdcLTIGEN ZEICHEN)\r\nN10 G90\r\nN20 G00 X10 Y10 \x1b\r\nN30 M30\r\n"


class ScenarioKind(StrEnum):
    SEND = "send"
    RECEIVE = "receive"
    BOTH = "both"


@dataclass(frozen=True)
class Scenario:
    id: str
    title: str
    description: str
    kind: ScenarioKind
    config: SimulationConfig
    sample_program: bytes = SAMPLE_PROGRAM
    expected_send_state: str | None = None
    expected_receive_state: str | None = None
    auto_cancel_after_bytes: int | None = None
    protocol: ProtocolMode = ProtocolMode.PLAIN
    lsv2: Lsv2SimConfig | None = None
    expected_drip_state: str | None = None


def _receive_program(data: bytes, **kwargs: Any) -> SimulationConfig:
    return SimulationConfig(machine_program=data, **kwargs)


SCENARIOS: tuple[Scenario, ...] = (
    Scenario(
        "send_ok",
        "Erfolgreiches Senden",
        "Die simulierte CNC nimmt alle Daten an, CTS und DSR sind aktiv.",
        ScenarioKind.SEND,
        SimulationConfig(),
        expected_send_state="completed",
    ),
    Scenario(
        "receive_ok",
        "Erfolgreicher Empfang",
        "Die simulierte CNC sendet ein vollständiges Programm mit M30.",
        ScenarioKind.RECEIVE,
        _receive_program(SAMPLE_PROGRAM),
        expected_receive_state="completed",
    ),
    Scenario(
        "cts_off",
        "CTS bleibt aus",
        "Die Gegenstelle erteilt nie die Sendefreigabe (CTS). Bei Profilen mit CTS-Prüfung "
        "endet das Senden mit Handshake-Timeout.",
        ScenarioKind.SEND,
        SimulationConfig(cts=False),
        expected_send_state="failed",
    ),
    Scenario(
        "send_timeout",
        "Timeout während des Sendens",
        "Nach 100 Bytes nimmt die CNC keine Daten mehr an.",
        ScenarioKind.SEND,
        SimulationConfig(write_stall_after_bytes=100),
        expected_send_state="failed",
    ),
    Scenario(
        "user_cancel",
        "Abbruch durch Benutzer",
        "Langsame Übertragung - brechen Sie manuell ab. Im automatischen Test wird nach 100 Bytes abgebrochen.",
        ScenarioKind.BOTH,
        _receive_program(SAMPLE_PROGRAM * 4, speed_factor=0.25),
        expected_send_state="cancelled",
        expected_receive_state="cancelled",
        auto_cancel_after_bytes=100,
    ),
    Scenario(
        "receive_no_m30",
        "Empfang ohne M30",
        "Die CNC sendet ein Programm ohne Programmende; der Empfang endet per Inaktivität "
        "und wird als unvollständig gekennzeichnet.",
        ScenarioKind.RECEIVE,
        _receive_program(SAMPLE_PROGRAM.replace(b"N110 M30\r\n", b"")),
        expected_receive_state="completed",
    ),
    Scenario(
        "receive_nulls",
        "Empfang mit Nullbytes",
        "Im Datenstrom sind Nullbytes enthalten (z. B. Vorlauf/Leader).",
        ScenarioKind.RECEIVE,
        _receive_program(b"\x00" * 20 + SAMPLE_PROGRAM, inject_null_every=37),
        expected_receive_state="completed",
    ),
    Scenario(
        "disconnect",
        "Verbindungsabbruch während des Transfers",
        "Die Verbindung bricht nach 150 Bytes ab (Senden und Empfangen).",
        ScenarioKind.BOTH,
        _receive_program(SAMPLE_PROGRAM, disconnect_after_bytes=150),
        expected_send_state="failed",
        expected_receive_state="failed",
    ),
    Scenario(
        "invalid_ascii_file",
        "Datei mit ungültigen ASCII-Zeichen",
        "Das Beispielprogramm enthält Umlaute und Steuerzeichen; die Prüfung sperrt die Übertragung.",
        ScenarioKind.SEND,
        SimulationConfig(),
        sample_program=INVALID_PROGRAM,
    ),
    Scenario(
        "slow",
        "Langsame Übertragung",
        "Übertragung mit einem Fünftel der Baudrate, empfangen in kleinen Paketen.",
        ScenarioKind.BOTH,
        _receive_program(SAMPLE_PROGRAM, speed_factor=0.2, read_chunk_sizes=(1, 3, 7)),
        expected_send_state="completed",
        expected_receive_state="completed",
    ),
    Scenario(
        "partial_io",
        "Partielle Reads und Writes",
        "Der Treiber nimmt höchstens 5 Bytes pro Schreibaufruf an und liefert variable Paketgrößen.",
        ScenarioKind.BOTH,
        _receive_program(SAMPLE_PROGRAM, max_write_chunk=5, read_chunk_sizes=(1, 2, 13, 64)),
        expected_send_state="completed",
        expected_receive_state="completed",
    ),
    Scenario(
        "receive_mixed_endings",
        "Empfang mit gemischten Zeilenenden und zusätzlichem CRLF",
        "Zeilenenden CR, LF und CRLF gemischt, nach M30 folgt ein zusätzliches CRLF.",
        ScenarioKind.RECEIVE,
        _receive_program(
            SAMPLE_PROGRAM.replace(b"N20 G00 X0 Y0 Z50\r\n", b"N20 G00 X0 Y0 Z50\n").replace(
                b"N40 S1200 M03\r\n", b"N40 S1200 M03\r"
            )
            + b"\r\n"
        ),
        expected_receive_state="completed",
    ),
    Scenario(
        "receive_corrupted",
        "Empfang mit beschädigten Bytes",
        "Einzelne Bytes werden verfälscht (simulierter Paritäts-/Störungsfehler).",
        ScenarioKind.RECEIVE,
        _receive_program(SAMPLE_PROGRAM, corrupt_offsets=(5, 60, 120)),
        expected_receive_state="completed",
    ),
    Scenario(
        "port_open_error",
        "Port nicht verfügbar",
        "Simuliert einen belegten Anschluss beim Öffnen.",
        ScenarioKind.BOTH,
        SimulationConfig(open_error="busy"),
        expected_send_state="failed",
        expected_receive_state="failed",
    ),
    Scenario(
        "write_error",
        "Schreibfehler",
        "Nach 80 Bytes meldet der Treiber einen Schreibfehler.",
        ScenarioKind.SEND,
        SimulationConfig(write_error_after_bytes=80),
        expected_send_state="failed",
    ),
    # ---- LSV2 scenarios (only effective with an LSV2 profile) ----------------------
    Scenario(
        "lsv2_ok",
        "LSV2: Erfolgreiche Übertragung",
        "LSV2-CNC quittiert jedes Telegramm sofort und sendet beim Empfang ein Programm. Nur mit LSV2-Profil wirksam.",
        ScenarioKind.BOTH,
        SimulationConfig(),
        protocol=ProtocolMode.LSV2,
        lsv2=Lsv2SimConfig(program=SAMPLE_PROGRAM),
        expected_send_state="completed",
        expected_receive_state="completed",
        expected_drip_state="completed",
    ),
    Scenario(
        "lsv2_dripfeed",
        "LSV2: Nachladebetrieb (Puffer 3 Sätze, 0,5 s je Satz)",
        "Die simulierte CNC arbeitet das Programm ab und quittiert erst, wenn im Satzpuffer "
        "Platz ist. Nur mit LSV2-Profil wirksam.",
        ScenarioKind.SEND,
        SimulationConfig(),
        protocol=ProtocolMode.LSV2,
        lsv2=Lsv2SimConfig(buffer_telegrams=3, execution_s_per_telegram=0.5),
        expected_send_state="completed",
        expected_drip_state="completed",
    ),
    Scenario(
        "lsv2_nak",
        "LSV2: Prüfsummenfehler / NAK",
        "Die CNC antwortet auf Telegramm 2 und 5 einmal mit NAK, beim Senden der CNC ist "
        "Telegramm 3 beschädigt. Die Übertragung muss durch Wiederholung gelingen.",
        ScenarioKind.BOTH,
        SimulationConfig(),
        protocol=ProtocolMode.LSV2,
        lsv2=Lsv2SimConfig(nak_on_telegrams=(2, 5), corrupt_bcc_on_sent_telegrams=(3,), program=SAMPLE_PROGRAM),
        expected_send_state="completed",
        expected_receive_state="completed",
        expected_drip_state="completed",
    ),
    Scenario(
        "lsv2_lost_ack",
        "LSV2: Quittung geht verloren",
        "Die Quittung für Telegramm 4 geht verloren; DeckelLink fragt mit ENQ nach, ohne das "
        "Telegramm doppelt zu senden.",
        ScenarioKind.SEND,
        SimulationConfig(),
        protocol=ProtocolMode.LSV2,
        lsv2=Lsv2SimConfig(drop_ack_on_telegrams=(4,)),
        expected_send_state="completed",
        expected_drip_state="completed",
    ),
    Scenario(
        "lsv2_wack",
        "LSV2: CNC meldet 'Warten' (WACK)",
        "Die CNC antwortet bei Telegramm 2 und 3 mit WACK; DeckelLink wartet und fragt nach.",
        ScenarioKind.SEND,
        SimulationConfig(),
        protocol=ProtocolMode.LSV2,
        lsv2=Lsv2SimConfig(wack_on_telegrams=(2, 3)),
        expected_send_state="completed",
        expected_drip_state="completed",
    ),
    Scenario(
        "lsv2_cnc_abort",
        "LSV2: CNC bricht ab (EOT)",
        "Nach Telegramm 5 beendet die CNC die Verbindung mit EOT (z. B. Programmabbruch an der Steuerung).",
        ScenarioKind.SEND,
        SimulationConfig(),
        protocol=ProtocolMode.LSV2,
        lsv2=Lsv2SimConfig(eot_after_telegrams=5),
        expected_send_state="failed",
        expected_drip_state="failed",
    ),
    Scenario(
        "lsv2_not_ready",
        "LSV2: CNC nicht empfangsbereit",
        "Die CNC beantwortet ENQ nie - Verbindungsaufbau scheitert nach der Wartezeit.",
        ScenarioKind.SEND,
        SimulationConfig(),
        protocol=ProtocolMode.LSV2,
        lsv2=Lsv2SimConfig(never_ready=True),
        expected_send_state="failed",
        expected_drip_state="failed",
    ),
    Scenario(
        "lsv2_disconnect",
        "LSV2: Verbindungsabbruch im Nachladebetrieb",
        "Nach Telegramm 4 bricht die Verbindung ab.",
        ScenarioKind.SEND,
        SimulationConfig(),
        protocol=ProtocolMode.LSV2,
        lsv2=Lsv2SimConfig(disconnect_after_telegrams=4, buffer_telegrams=2, execution_s_per_telegram=0.2),
        expected_send_state="failed",
        expected_drip_state="failed",
    ),
)

REPLAY_SCENARIO_ID = "replay"
SCENARIOS = (
    *SCENARIOS,
    Scenario(
        REPLAY_SCENARIO_ID,
        "Aufzeichnung abspielen (echte Rohdaten)",
        "Die simulierte CNC sendet beim Empfang (und beim Mitlesen des Datenformats) die Bytes einer "
        "gewählten Aufzeichnung, z. B. einer Rohdaten-Datei aus dem Ordner 'raw' nach dem ersten echten "
        "Empfang. Bei LSV2-Profilen wird der Inhalt als Telegramme neu erzeugt.",
        ScenarioKind.RECEIVE,
        SimulationConfig(),
    ),
)
SCENARIOS_BY_ID = {s.id: s for s in SCENARIOS}
DEFAULT_SCENARIO_ID = "send_ok"


def get_scenario(scenario_id: str | None) -> Scenario:
    return SCENARIOS_BY_ID.get(scenario_id or "", SCENARIOS_BY_ID[DEFAULT_SCENARIO_ID])


def lsv2_config_for(scenario: Scenario, *, receive: bool = False, drip_feed: bool = False) -> Lsv2SimConfig:
    """LSV2 simulator settings for any scenario (plain scenarios get sensible LSV2 defaults)."""
    if scenario.lsv2 is not None:
        config = scenario.lsv2
    else:
        config = Lsv2SimConfig(open_error=scenario.config.open_error)
        if drip_feed:
            config = replace(config, buffer_telegrams=4, execution_s_per_telegram=0.2)
    if receive and not config.program:
        config = replace(config, program=SAMPLE_PROGRAM)
    if not receive:
        # When the PC sends, the simulated CNC is the receiver.
        config = replace(config, program=b"")
    return config
