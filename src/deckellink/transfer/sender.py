"""Sending NC data to the CNC.

The sender only transmits bytes. It never sends execution commands and never
toggles anything that could start a program on the machine.
"""

from __future__ import annotations

import logging
from collections.abc import Callable
from dataclasses import dataclass

from deckellink.clock import Clock, SystemClock
from deckellink.diagnostics.error_messages import describe_error
from deckellink.domain.errors import UserFacingError
from deckellink.domain.models import Profile
from deckellink.domain.results import (
    Direction,
    SendResult,
    TrafficEvent,
    TransferProgress,
    TransformResult,
)
from deckellink.domain.states import SendState
from deckellink.storage.checksums import sha256_bytes
from deckellink.transfer.cancellation import CancellationToken, OperationCancelled
from deckellink.transfer.progress import ProgressTracker
from deckellink.transport.base import (
    HandshakeTimeoutError,
    ModemStatus,
    Transport,
    TransportTimeoutError,
)

log = logging.getLogger(__name__)

DEFAULT_CHUNK = 64
POLL_INTERVAL_S = 0.05


@dataclass
class SendCallbacks:
    on_state: Callable[[SendState], None] | None = None
    on_progress: Callable[[TransferProgress], None] | None = None
    on_traffic: Callable[[TrafficEvent], None] | None = None
    on_modem: Callable[[ModemStatus], None] | None = None


class ProgramSender:
    def __init__(
        self,
        transport: Transport,
        profile: Profile,
        *,
        clock: Clock | None = None,
        token: CancellationToken | None = None,
        callbacks: SendCallbacks | None = None,
    ) -> None:
        self.transport = transport
        self.profile = profile
        self.clock = clock or SystemClock()
        self.token = token or CancellationToken()
        self.callbacks = callbacks or SendCallbacks()
        self.state = SendState.READY

    # -- helpers -----------------------------------------------------------
    def _set_state(self, state: SendState) -> None:
        self.state = state
        log.debug("Send state -> %s", state.value)
        if self.callbacks.on_state:
            self.callbacks.on_state(state)

    def _sleep_ms(self, milliseconds: int) -> None:
        if milliseconds <= 0:
            self.token.raise_if_cancelled()
            return
        if self.clock.sleep(milliseconds / 1000.0, self.token.event):
            raise OperationCancelled("Abbruch während Wartezeit")

    def _emit_modem(self) -> ModemStatus:
        status = self.transport.get_modem_status()
        if self.callbacks.on_modem:
            self.callbacks.on_modem(status)
        return status

    # -- main --------------------------------------------------------------
    def send(self, payload: TransformResult) -> SendResult:
        data = payload.data
        started_at = self.clock.now()
        t0 = self.clock.monotonic()
        tracker = ProgressTracker(payload, self.clock, started_at)
        serial = self.profile.serial
        error: UserFacingError | None = None
        port_left_open = False
        log.info(
            "Send start: profile=%s port=%s bytes=%d lines=%d sha256=%s simulated=%s",
            self.profile.id,
            self.transport.name,
            len(data),
            payload.line_count,
            sha256_bytes(data)[:16],
            self.transport.is_simulated,
        )
        try:
            self.token.raise_if_cancelled()
            self._set_state(SendState.OPENING_PORT)
            if not self.transport.is_open:
                self.transport.open()
            self.token.register(self.transport.cancel)
            self._emit_modem()

            if serial.require_cts_before_send or serial.require_dsr_before_send:
                self._set_state(SendState.WAITING_FOR_HANDSHAKE)
                self._wait_for_handshake()

            self._sleep_ms(self.profile.transfer.start_delay_ms)
            self._set_state(SendState.SENDING)
            tracker.start()
            if self.callbacks.on_progress:
                self.callbacks.on_progress(tracker.snapshot())
            self._send_bytes(data, tracker)

            self._set_state(SendState.FINISHING)
            self._drain()
            self._sleep_ms(self.profile.transfer.end_delay_ms)
            self._emit_modem()
            self._set_state(SendState.COMPLETED)
        except OperationCancelled:
            log.warning("Send cancelled by user after %d of %d bytes", tracker.bytes_done, len(data))
            self._set_state(SendState.CANCELLED)
        except Exception as exc:
            error = describe_error(exc, operation="Senden", port=self.transport.name, profile_id=self.profile.id)
            self._set_state(SendState.FAILED)
        finally:
            keep_open = self.state is SendState.COMPLETED and not self.profile.transfer.close_port_after_send
            if keep_open and self.transport.is_open:
                port_left_open = True
            else:
                self.transport.close()

        duration = self.clock.monotonic() - t0
        result = SendResult(
            state=self.state,
            bytes_total=len(data),
            bytes_sent=tracker.bytes_done,
            started_at=started_at,
            duration_s=duration,
            sha256=sha256_bytes(data),
            profile_id=self.profile.id,
            port=self.transport.name,
            simulated=self.transport.is_simulated,
            error=error,
            port_left_open=port_left_open,
        )
        log.info(
            "Send end: state=%s sent=%d/%d duration=%.2fs",
            result.state.value,
            result.bytes_sent,
            result.bytes_total,
            duration,
        )
        return result

    def _wait_for_handshake(self) -> None:
        serial = self.profile.serial
        deadline = self.clock.monotonic() + serial.handshake_timeout_ms / 1000.0
        while True:
            status = self._emit_modem()
            missing = []
            if serial.require_cts_before_send and not status.cts:
                missing.append("CTS")
            if serial.require_dsr_before_send and not status.dsr:
                missing.append("DSR")
            if not missing:
                log.info("Handshake ready (CTS=%s DSR=%s)", status.cts, status.dsr)
                return
            if self.clock.monotonic() >= deadline:
                raise HandshakeTimeoutError(
                    f"{'/'.join(missing)} nicht aktiv nach {serial.handshake_timeout_ms} ms",
                    self.transport.name,
                    tuple(missing),
                )
            if self.clock.sleep(POLL_INTERVAL_S, self.token.event):
                raise OperationCancelled("Abbruch während Handshake")

    def _send_bytes(self, data: bytes, tracker: ProgressTracker) -> None:
        settings = self.profile.transfer
        if settings.character_delay_ms > 0:
            unit = 1
        elif settings.packet_size > 0:
            unit = settings.packet_size
        else:
            unit = DEFAULT_CHUNK
        offset = 0
        total = len(data)
        while offset < total:
            self.token.raise_if_cancelled()
            end = min(offset + unit, total)
            self._write_all(data[offset:end], tracker)
            offset = end
            if offset >= total:
                break
            if settings.character_delay_ms > 0:
                self._sleep_ms(settings.character_delay_ms)
            if settings.packet_size > 0 and settings.packet_delay_ms > 0 and offset % settings.packet_size == 0:
                self._sleep_ms(settings.packet_delay_ms)

    def _write_all(self, block: bytes, tracker: ProgressTracker) -> None:
        timeout_s = self.profile.serial.write_timeout_ms / 1000.0
        pos = 0
        stall_since: float | None = None
        while pos < len(block):
            self.token.raise_if_cancelled()
            written = self.transport.write(block[pos:])
            self.token.raise_if_cancelled()
            if written > 0:
                chunk = block[pos : pos + written]
                pos += written
                stall_since = None
                if self.callbacks.on_traffic:
                    self.callbacks.on_traffic(TrafficEvent(Direction.TX, self.clock.now(), chunk))
                progress = tracker.advance(written)
                if self.callbacks.on_progress:
                    self.callbacks.on_progress(progress)
                continue
            now = self.clock.monotonic()
            if stall_since is None:
                stall_since = now
            elif now - stall_since >= timeout_s:
                raise TransportTimeoutError(f"Keine Daten mehr angenommen seit {timeout_s:.1f} s", self.transport.name)
            if self.clock.sleep(POLL_INTERVAL_S, self.token.event):
                raise OperationCancelled("Abbruch während Schreibvorgang")

    def _drain(self) -> None:
        """Wait until the driver's output buffer is empty (bounded by write timeout)."""
        timeout_s = self.profile.serial.write_timeout_ms / 1000.0
        deadline = self.clock.monotonic() + timeout_s
        while self.transport.out_waiting > 0:
            if self.clock.monotonic() >= deadline:
                raise TransportTimeoutError(
                    "Ausgangspuffer wurde nicht geleert (Handshake blockiert?)", self.transport.name
                )
            if self.clock.sleep(POLL_INTERVAL_S, self.token.event):
                raise OperationCancelled("Abbruch beim Abschließen")
