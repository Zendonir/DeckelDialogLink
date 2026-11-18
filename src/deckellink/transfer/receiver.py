"""Receiving NC data from the CNC."""

from __future__ import annotations

import logging
from collections.abc import Callable
from dataclasses import dataclass
from datetime import datetime

from deckellink.clock import Clock, SystemClock
from deckellink.diagnostics.error_messages import describe_error
from deckellink.domain.errors import UserFacingError
from deckellink.domain.models import LineEnding, Profile
from deckellink.domain.results import (
    Direction,
    ReceivedProgram,
    ReceiveProgress,
    ReceiveResult,
    TrafficEvent,
)
from deckellink.domain.states import EndReason, ReceiveState
from deckellink.nc.analyzer import analyze
from deckellink.nc.checks import program_end_pattern, split_lines
from deckellink.storage.checksums import sha256_bytes
from deckellink.transfer.cancellation import CancellationToken, OperationCancelled
from deckellink.transport.base import ModemStatus, Transport

log = logging.getLogger(__name__)

READ_SIZE = 256
SEARCH_OVERLAP = 64


@dataclass
class ReceiveCallbacks:
    on_state: Callable[[ReceiveState], None] | None = None
    on_progress: Callable[[ReceiveProgress], None] | None = None
    on_traffic: Callable[[TrafficEvent], None] | None = None
    on_modem: Callable[[ModemStatus], None] | None = None


class EndSequenceDetector:
    """Detects the configured end sequence (default ``M30``) plus optional line end.

    Works incrementally across chunk boundaries and ignores NUL bytes.
    """

    def __init__(self, sequence: str) -> None:
        self._pattern = program_end_pattern(sequence)
        self._scanned = 0
        self.match_end: int | None = None

    @property
    def enabled(self) -> bool:
        return self._pattern is not None

    def feed(self, buffer: bytes) -> bool:
        """Return True once the sequence was found anywhere in ``buffer``."""
        if self._pattern is None:
            return False
        if self.match_end is not None:
            return True
        start = max(0, self._scanned - SEARCH_OVERLAP)
        window = buffer[start:].replace(b"\x00", b"\x01")
        match = self._pattern.search(window)
        self._scanned = len(buffer)
        if match is None:
            return False
        # Make sure the token is complete (not followed by another digit later).
        if match.end() == len(window):
            self._scanned = start + match.start()
            return False
        self.match_end = start + match.end()
        return True

    def line_terminated(self, buffer: bytes, line_ending: LineEnding = LineEnding.CRLF) -> bool:
        """True if the line ending expected by the profile follows the detected sequence.

        For CRLF and LF the LF must have arrived; for CR a CR is sufficient. If the
        control sends a different terminator, the grace period ends the receive.
        """
        if self.match_end is None:
            return False
        tail = buffer[self.match_end :].replace(b"\x00", b"")
        if line_ending is LineEnding.CR:
            return b"\r" in tail or b"\n" in tail
        return b"\n" in tail


class ProgramReceiver:
    def __init__(
        self,
        transport: Transport,
        profile: Profile,
        *,
        clock: Clock | None = None,
        token: CancellationToken | None = None,
        callbacks: ReceiveCallbacks | None = None,
        read_size: int = READ_SIZE,
    ) -> None:
        self.transport = transport
        self.profile = profile
        self.clock = clock or SystemClock()
        self.token = token or CancellationToken()
        self.callbacks = callbacks or ReceiveCallbacks()
        self.read_size = read_size
        self.state = ReceiveState.IDLE

    def _set_state(self, state: ReceiveState) -> None:
        self.state = state
        log.debug("Receive state -> %s", state.value)
        if self.callbacks.on_state:
            self.callbacks.on_state(state)

    def receive(self) -> ReceiveResult:
        settings = self.profile.receive
        started_at = self.clock.now()
        t0 = self.clock.monotonic()
        buffer = bytearray()
        first_at: datetime | None = None
        last_at: datetime | None = None
        last_mono: float | None = None
        end_reason = EndReason.ERROR
        error: UserFacingError | None = None
        final_state = ReceiveState.FAILED
        detector = EndSequenceDetector(settings.end_sequence)
        end_seen_mono: float | None = None
        log.info("Receive start: profile=%s port=%s", self.profile.id, self.transport.name)
        try:
            self.token.raise_if_cancelled()
            self._set_state(ReceiveState.OPENING_PORT)
            if not self.transport.is_open:
                self.transport.open()
            self.token.register(self.transport.cancel)
            if self.callbacks.on_modem:
                self.callbacks.on_modem(self.transport.get_modem_status())
            self._set_state(ReceiveState.LISTENING)
            listen_start = self.clock.monotonic()
            while True:
                self.token.raise_if_cancelled()
                chunk = self.transport.read(self.read_size)
                self.token.raise_if_cancelled()
                now = self.clock.monotonic()
                if chunk:
                    wall = self.clock.now()
                    if first_at is None:
                        first_at = wall
                        self._set_state(ReceiveState.RECEIVING)
                    last_at = wall
                    last_mono = now
                    if settings.max_receive_bytes > 0 and len(buffer) + len(chunk) > settings.max_receive_bytes:
                        room = settings.max_receive_bytes - len(buffer)
                        buffer += chunk[:room]
                        self._traffic(chunk[:room], wall)
                        end_reason = EndReason.MAX_SIZE
                        final_state = ReceiveState.FAILED
                        error = UserFacingError(
                            title="Maximale Empfangsgröße überschritten - Empfang gestoppt.",
                            causes=(
                                f"Grenze laut Profil: {settings.max_receive_bytes} Bytes.",
                                "Die CNC sendet mehr Daten als erwartet oder sendet fortlaufend.",
                            ),
                            technical_details=f"received>{settings.max_receive_bytes}",
                            operation="Empfangen",
                        )
                        log.warning("Receive stopped: max size %d reached", settings.max_receive_bytes)
                        break
                    buffer += chunk
                    self._traffic(chunk, wall)
                    self._progress(len(buffer), now - t0, 0.0)
                    if (
                        settings.end_detection.uses_end_sequence
                        and end_seen_mono is None
                        and detector.feed(bytes(buffer))
                    ):
                        end_seen_mono = now
                        log.info("End sequence '%s' detected", settings.end_sequence)
                    if end_seen_mono is not None and detector.line_terminated(
                        bytes(buffer), self.profile.transfer.line_ending
                    ):
                        end_reason = EndReason.END_SEQUENCE
                        final_state = ReceiveState.COMPLETED
                        break
                    if end_seen_mono is not None and now - end_seen_mono >= settings.end_grace_ms / 1000.0:
                        end_reason = EndReason.END_SEQUENCE
                        final_state = ReceiveState.COMPLETED
                        break
                    continue

                if first_at is None or last_mono is None:
                    if (
                        settings.first_byte_timeout_ms > 0
                        and now - listen_start >= settings.first_byte_timeout_ms / 1000.0
                    ):
                        end_reason = EndReason.NO_DATA
                        final_state = ReceiveState.TIMED_OUT
                        log.warning("Receive timed out: no data within %d ms", settings.first_byte_timeout_ms)
                        break
                    self._progress(0, now - t0, now - listen_start)
                    continue
                idle = now - last_mono
                self._progress(len(buffer), now - t0, idle)
                if end_seen_mono is not None and now - end_seen_mono >= settings.end_grace_ms / 1000.0:
                    end_reason = EndReason.END_SEQUENCE
                    final_state = ReceiveState.COMPLETED
                    break
                if idle >= settings.inactivity_timeout_ms / 1000.0:
                    end_reason = EndReason.INACTIVITY
                    final_state = (
                        ReceiveState.COMPLETED if settings.end_detection.uses_inactivity else ReceiveState.TIMED_OUT
                    )
                    break
            self._set_state(ReceiveState.FINALIZING)
        except OperationCancelled:
            end_reason = EndReason.CANCELLED
            final_state = ReceiveState.CANCELLED
            log.warning("Receive cancelled by user after %d bytes", len(buffer))
        except Exception as exc:
            end_reason = EndReason.ERROR
            final_state = ReceiveState.FAILED
            error = describe_error(exc, operation="Empfangen", port=self.transport.name, profile_id=self.profile.id)
        finally:
            self.transport.close()

        self._set_state(final_state)
        raw = bytes(buffer)
        duration = self.clock.monotonic() - t0
        log.info(
            "Receive end: state=%s reason=%s bytes=%d sha256=%s duration=%.2fs",
            final_state.value,
            end_reason.value,
            len(raw),
            sha256_bytes(raw)[:16],
            duration,
        )
        return ReceiveResult(
            state=final_state,
            raw=raw,
            end_reason=end_reason,
            started_at=started_at,
            duration_s=duration,
            profile_id=self.profile.id,
            port=self.transport.name,
            simulated=self.transport.is_simulated,
            first_byte_at=first_at,
            last_byte_at=last_at,
            error=error,
        )

    def _traffic(self, data: bytes, when: datetime) -> None:
        if data and self.callbacks.on_traffic:
            self.callbacks.on_traffic(TrafficEvent(Direction.RX, when, data))

    def _progress(self, count: int, elapsed: float, idle: float) -> None:
        if self.callbacks.on_progress:
            self.callbacks.on_progress(ReceiveProgress(self.state, count, elapsed, idle))


def normalize_received(raw: bytes, profile: Profile) -> bytes:
    settings = profile.receive
    data = raw.replace(b"\x00", b"") if settings.strip_null_bytes else raw
    if settings.normalize_line_endings:
        target = profile.transfer.line_ending.sequence
        parts = []
        for line in split_lines(data):
            parts.append(line.content + (target if line.ending else b""))
        data = b"".join(parts)
    return data


def process_received(result: ReceiveResult, profile: Profile) -> ReceivedProgram:
    """Normalise and analyse received raw bytes. The raw bytes stay untouched."""
    raw = result.raw
    normalized = normalize_received(raw, profile)
    analysis = analyze(normalized, profile.transfer, profile.serial)
    decode_errors = sum(1 for b in raw if b > 0x7F)
    notes: list[str] = []
    if decode_errors:
        notes.append(
            f"{decode_errors} Byte(s) außerhalb 7-Bit-ASCII empfangen - möglicher Paritäts- oder "
            "Datenbitfehler (Profil prüfen)."
        )
    if b"\x00" in raw:
        notes.append(
            f"{raw.count(0)} Nullbyte(s) im Rohdatenstrom"
            + (" - in der NC-Datei entfernt." if profile.receive.strip_null_bytes else ".")
        )
    complete = (
        result.state is ReceiveState.COMPLETED
        and result.end_reason in (EndReason.END_SEQUENCE, EndReason.INACTIVITY)
        and (analysis.has_program_end or not profile.transfer.program_end.strip())
    )
    if not complete:
        notes.append("Programm ist möglicherweise UNVOLLSTÄNDIG - nicht ungeprüft verwenden.")
    if profile.transfer.line_ending is not LineEnding.CRLF and profile.receive.normalize_line_endings:
        notes.append(f"Zeilenenden auf {profile.transfer.line_ending.value.upper()} normalisiert.")
    return ReceivedProgram(
        raw=raw,
        normalized=normalized,
        analysis=analysis,
        complete=complete,
        decode_errors=decode_errors,
        notes=tuple(notes),
    )
