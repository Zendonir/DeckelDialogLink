"""Human-readable modem line descriptions."""

from __future__ import annotations

from deckellink.transport.base import ModemStatus

SIGNAL_DESCRIPTIONS = {
    "CTS": "Clear To Send - Sendefreigabe der Gegenstelle (Eingang)",
    "DSR": "Data Set Ready - Gegenstelle betriebsbereit (Eingang)",
    "DCD": "Data Carrier Detect - Trägersignal (Eingang)",
    "RI": "Ring Indicator (Eingang)",
    "RTS": "Request To Send - eigene Empfangsbereitschaft/Sendewunsch (Ausgang)",
    "DTR": "Data Terminal Ready - eigene Betriebsbereitschaft (Ausgang)",
}

INPUT_SIGNALS = ("CTS", "DSR", "DCD", "RI")
OUTPUT_SIGNALS = ("RTS", "DTR")


def signal_text(value: bool | None) -> str:
    if value is None:
        return "unbekannt"
    return "EIN" if value else "AUS"


def format_modem_status(status: ModemStatus) -> str:
    return "  ".join(f"{name}={signal_text(value)}" for name, value in status.as_dict().items())
