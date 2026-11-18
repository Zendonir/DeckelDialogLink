"""Translate exceptions into understandable German messages."""

from __future__ import annotations

import logging

from deckellink.domain.errors import ProfileValidationError, UserFacingError, ValidationBlockedError
from deckellink.nc.transformer import TransformError
from deckellink.storage.files import FileExistsConflict
from deckellink.transport.base import (
    ConnectionLostError,
    HandshakeTimeoutError,
    PortBusyError,
    PortNotFoundError,
    PortOpenError,
    TransportReadError,
    TransportTimeoutError,
    TransportWriteError,
)

log = logging.getLogger(__name__)

_PORT_OPEN_CAUSES = (
    "Der Anschluss wird bereits von einem anderen Programm verwendet.",
    "Der USB-RS-232-Adapter wurde getrennt.",
    "Der Gerätetreiber ist nicht installiert.",
    "Der ausgewählte COM-Port existiert nicht mehr.",
)


def describe_error(
    exc: BaseException,
    *,
    operation: str = "",
    port: str = "",
    profile_id: str = "",
) -> UserFacingError:
    """Build a :class:`UserFacingError` and log the technical context."""
    port_label = port or getattr(exc, "port", "") or "Der Port"
    title: str
    causes: tuple[str, ...]
    if isinstance(exc, PortBusyError):
        title = f"{port_label} konnte nicht geöffnet werden - der Anschluss ist belegt."
        causes = (
            "Der Anschluss wird bereits von einem anderen Programm verwendet (z. B. Terminalprogramm, DNC-Software).",
            "Eine frühere DeckelLink-Instanz läuft noch.",
            "Fehlende Zugriffsrechte auf den Anschluss.",
        )
    elif isinstance(exc, PortNotFoundError):
        title = f"{port_label} wurde nicht gefunden."
        causes = (
            "Der USB-RS-232-Adapter wurde getrennt.",
            "Der ausgewählte COM-Port existiert nicht mehr (andere COM-Nummer nach Neustecken).",
            "Der Gerätetreiber ist nicht installiert.",
        )
    elif isinstance(exc, PortOpenError):
        title = f"{port_label} konnte nicht geöffnet werden."
        causes = _PORT_OPEN_CAUSES
    elif isinstance(exc, HandshakeTimeoutError):
        signals = "/".join(exc.missing) or "CTS"
        title = f"Die Gegenstelle hat die Sendefreigabe ({signals}) nicht erteilt."
        causes = (
            "Die CNC ist nicht empfangsbereit (Einlesen an der Steuerung nicht gestartet).",
            f"Das Kabel führt die Leitung {signals} nicht oder ist anders belegt.",
            "Das gewählte Profil passt nicht zur Handshake-Einstellung der Steuerung.",
            "Der USB-RS-232-Adapter unterstützt die Modemleitungen nicht.",
        )
    elif isinstance(exc, TransportTimeoutError):
        title = "Zeitüberschreitung bei der Übertragung."
        causes = (
            "Die Steuerung hat den Empfang angehalten (Handshake blockiert).",
            "Die CNC ist nicht mehr empfangsbereit oder der Speicher ist voll.",
            "Falsche Baudrate oder falsches Datenformat.",
            "Kabel oder Adapter defekt.",
        )
    elif isinstance(exc, ConnectionLostError):
        title = "Die Verbindung wurde während der Übertragung unterbrochen."
        causes = (
            "Der USB-RS-232-Adapter wurde getrennt oder hat die Verbindung verloren.",
            "Das Kabel wurde gelöst.",
            "Der Treiber hat den Anschluss zurückgesetzt (Energiesparmodus des USB-Ports).",
        )
    elif isinstance(exc, TransportWriteError):
        title = "Beim Senden ist ein Schreibfehler aufgetreten."
        causes = ("Adapter- oder Treiberfehler.", "Die Verbindung wurde unterbrochen.")
    elif isinstance(exc, TransportReadError):
        title = "Beim Empfangen ist ein Lesefehler aufgetreten."
        causes = ("Adapter- oder Treiberfehler.", "Die Verbindung wurde unterbrochen.")
    elif isinstance(exc, ProfileValidationError):
        title = "Das Profil ist ungültig."
        causes = tuple(str(e) for e in exc.errors)
    elif isinstance(exc, TransformError):
        title = "Die Datei kann mit dem gewählten Profil nicht für die Übertragung aufbereitet werden."
        causes = (str(exc),)
    elif isinstance(exc, ValidationBlockedError):
        title = "Die Übertragung ist wegen Prüffehlern gesperrt."
        causes = tuple(exc.reasons)
    elif isinstance(exc, FileExistsConflict):
        title = "Die Datei existiert bereits und wurde nicht überschrieben."
        causes = (str(exc),)
    elif isinstance(exc, PermissionError):
        title = "Zugriff auf eine Datei wurde verweigert."
        causes = (
            "Die Datei ist in einem anderen Programm geöffnet.",
            "Fehlende Schreibrechte im Zielordner.",
        )
    elif isinstance(exc, OSError):
        title = "Ein Datei- oder Systemfehler ist aufgetreten."
        causes = ("Datenträger voll oder nicht erreichbar.", "Fehlende Zugriffsrechte.")
    else:
        title = f"Unerwarteter Fehler{(' bei: ' + operation) if operation else ''}."
        causes = ("Bitte Diagnosepaket exportieren und den Fehler melden.",)

    technical = f"{type(exc).__name__}: {exc}"
    context = ", ".join(
        part
        for part in (
            f"Vorgang: {operation}" if operation else "",
            f"Port: {port}" if port else "",
            f"Profil: {profile_id}" if profile_id else "",
        )
        if part
    )
    if context:
        technical = f"{technical}\n{context}"
    log.error(
        "Error during %s (port=%s, profile=%s): %s: %s",
        operation or "operation",
        port or "-",
        profile_id or "-",
        type(exc).__name__,
        exc,
    )
    log.debug("Traceback for %s", type(exc).__name__, exc_info=exc)
    return UserFacingError(
        title=title,
        causes=causes,
        technical_details=technical,
        operation=operation,
        exception_type=type(exc).__name__,
    )
