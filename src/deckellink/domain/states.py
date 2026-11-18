"""State machines for sending and receiving."""

from __future__ import annotations

from enum import StrEnum


class SendState(StrEnum):
    IDLE = "idle"
    VALIDATING = "validating"
    READY = "ready"
    OPENING_PORT = "opening_port"
    WAITING_FOR_HANDSHAKE = "waiting_for_handshake"
    SENDING = "sending"
    DRIP_FEEDING = "drip_feeding"
    FINISHING = "finishing"
    COMPLETED = "completed"
    CANCELLED = "cancelled"
    FAILED = "failed"

    @property
    def label(self) -> str:
        return _SEND_LABELS[self]

    @property
    def is_terminal(self) -> bool:
        return self in (SendState.COMPLETED, SendState.CANCELLED, SendState.FAILED)

    @property
    def is_active(self) -> bool:
        return self in (
            SendState.OPENING_PORT,
            SendState.WAITING_FOR_HANDSHAKE,
            SendState.SENDING,
            SendState.DRIP_FEEDING,
            SendState.FINISHING,
        )


_SEND_LABELS = {
    SendState.IDLE: "Bereit - keine Datei",
    SendState.VALIDATING: "Datei wird geprüft",
    SendState.READY: "Bereit zum Senden",
    SendState.OPENING_PORT: "Port wird geöffnet",
    SendState.WAITING_FOR_HANDSHAKE: "Warte auf Freigabe (Handshake)",
    SendState.SENDING: "Sende Daten",
    SendState.DRIP_FEEDING: "Nachladebetrieb - Daten werden nach Quittung der CNC geliefert",
    SendState.FINISHING: "Übertragung wird abgeschlossen",
    SendState.COMPLETED: "Übertragung abgeschlossen",
    SendState.CANCELLED: "Übertragung abgebrochen",
    SendState.FAILED: "Übertragung fehlgeschlagen",
}


class ReceiveState(StrEnum):
    IDLE = "idle"
    OPENING_PORT = "opening_port"
    LISTENING = "listening"
    RECEIVING = "receiving"
    FINALIZING = "finalizing"
    COMPLETED = "completed"
    CANCELLED = "cancelled"
    TIMED_OUT = "timed_out"
    FAILED = "failed"

    @property
    def label(self) -> str:
        return _RECEIVE_LABELS[self]

    @property
    def is_terminal(self) -> bool:
        return self in (
            ReceiveState.COMPLETED,
            ReceiveState.CANCELLED,
            ReceiveState.TIMED_OUT,
            ReceiveState.FAILED,
        )

    @property
    def is_active(self) -> bool:
        return self in (
            ReceiveState.OPENING_PORT,
            ReceiveState.LISTENING,
            ReceiveState.RECEIVING,
            ReceiveState.FINALIZING,
        )


_RECEIVE_LABELS = {
    ReceiveState.IDLE: "Bereit",
    ReceiveState.OPENING_PORT: "Port wird geöffnet",
    ReceiveState.LISTENING: "Empfangsbereit - warte auf Daten der CNC",
    ReceiveState.RECEIVING: "Empfange Daten",
    ReceiveState.FINALIZING: "Empfang wird abgeschlossen",
    ReceiveState.COMPLETED: "Empfang abgeschlossen",
    ReceiveState.CANCELLED: "Empfang abgebrochen",
    ReceiveState.TIMED_OUT: "Zeitüberschreitung beim Empfang",
    ReceiveState.FAILED: "Empfang fehlgeschlagen",
}


class EndReason(StrEnum):
    """Why a receive operation ended."""

    END_SEQUENCE = "end_sequence"
    INACTIVITY = "inactivity"
    MAX_SIZE = "max_size"
    NO_DATA = "no_data"
    CANCELLED = "cancelled"
    ERROR = "error"
    EOT = "eot"

    @property
    def label(self) -> str:
        return {
            "end_sequence": "Endsequenz erkannt",
            "inactivity": "Inaktivitätszeit abgelaufen",
            "max_size": "Maximale Empfangsgröße erreicht",
            "no_data": "Keine Daten empfangen",
            "cancelled": "Durch Benutzer abgebrochen",
            "error": "Übertragungsfehler",
            "eot": "Übertragungsende (EOT) der CNC",
        }[self.value]
