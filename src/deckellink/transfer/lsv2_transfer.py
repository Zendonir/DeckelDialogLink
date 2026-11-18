"""Program transfer and NC drip-feed ("Nachladebetrieb") over LSV2.

Safety boundary: in drip-feed mode the CNC requests data by acknowledging each
telegram while it executes the program. DeckelLink only *delivers* data; the
program is selected and started exclusively by the operator at the CNC.
Stopping the data flow does not stop the machine - the operator must stop it
at the control.
"""

from __future__ import annotations

import logging

from deckellink.clock import Clock, SystemClock
from deckellink.diagnostics.error_messages import describe_error
from deckellink.domain.errors import UserFacingError
from deckellink.domain.models import Profile
from deckellink.domain.results import ReceiveProgress, ReceiveResult, SendResult, TransformResult
from deckellink.domain.states import EndReason, ReceiveState, SendState
from deckellink.storage.checksums import sha256_bytes
from deckellink.transfer.cancellation import CancellationToken, OperationCancelled
from deckellink.transfer.lsv2 import EventKind, Lsv2Error, Lsv2Link
from deckellink.transfer.progress import ProgressTracker
from deckellink.transfer.receiver import ReceiveCallbacks
from deckellink.transfer.sender import POLL_INTERVAL_S, SendCallbacks
from deckellink.transport.base import HandshakeTimeoutError, Transport

log = logging.getLogger(__name__)


def build_telegrams(payload: TransformResult, profile: Profile) -> list[tuple[bytes, int]]:
    """Split the transfer data into telegram texts.

    Returns ``(text, end_offset)`` pairs; ``end_offset`` is the byte offset in
    ``payload.data`` reached after the telegram (for progress and line display).
    """
    p = profile.protocol
    data = payload.data
    telegrams: list[tuple[bytes, int]] = []
    offsets = payload.line_end_offsets
    start = 0
    for index in range(0, len(offsets), p.lines_per_telegram):
        end = offsets[min(index + p.lines_per_telegram, len(offsets)) - 1]
        text = data[start:end]
        if not p.include_line_ending:
            text = b"".join(line for line in text.replace(b"\r\n", b"\n").replace(b"\r", b"\n").split(b"\n") if line)
        if len(text) > p.max_telegram_length:
            line = payload.line_for_offset(start + 1)
            raise Lsv2Error(
                f"Telegramm ab Zeile {line} ist {len(text)} Zeichen lang (Profilgrenze {p.max_telegram_length})."
            )
        telegrams.append((text, end))
        start = end
    return telegrams


def _template(text: str, name: str) -> bytes:
    return text.replace("{name}", name).encode("ascii")


class Lsv2ProgramSender:
    """Sends a program (memory transfer) or feeds it on demand (drip-feed)."""

    def __init__(
        self,
        transport: Transport,
        profile: Profile,
        *,
        drip_feed: bool = False,
        program_name: str = "PROGRAMM",
        clock: Clock | None = None,
        token: CancellationToken | None = None,
        callbacks: SendCallbacks | None = None,
    ) -> None:
        self.transport = transport
        self.profile = profile
        self.drip_feed = drip_feed
        self.program_name = program_name
        self.clock = clock or SystemClock()
        self.token = token or CancellationToken()
        self.callbacks = callbacks or SendCallbacks()
        self.state = SendState.READY
        self.link: Lsv2Link | None = None

    def _set_state(self, state: SendState) -> None:
        self.state = state
        if self.callbacks.on_state:
            self.callbacks.on_state(state)

    def send(self, payload: TransformResult) -> SendResult:
        started_at = self.clock.now()
        t0 = self.clock.monotonic()
        tracker = ProgressTracker(payload, self.clock, started_at)
        error: UserFacingError | None = None
        operation = "Nachladebetrieb (LSV2)" if self.drip_feed else "Senden (LSV2)"
        p = self.profile.protocol
        log.info(
            "LSV2 %s start: profile=%s port=%s bytes=%d lines=%d sha256=%s",
            "drip-feed" if self.drip_feed else "send",
            self.profile.id,
            self.transport.name,
            len(payload.data),
            payload.line_count,
            sha256_bytes(payload.data)[:16],
        )
        link = Lsv2Link(self.transport, p, self.clock, self.token, self.callbacks.on_traffic)
        self.link = link
        try:
            telegrams = build_telegrams(payload, self.profile)
            self.token.raise_if_cancelled()
            self._set_state(SendState.OPENING_PORT)
            if not self.transport.is_open:
                self.transport.open()
            self._emit_modem()
            self._set_state(SendState.WAITING_FOR_HANDSHAKE)
            self._wait_lines()
            link.establish()
            ack_timeout = self._ack_timeout()
            for template in p.start_telegrams:
                link.send_telegram(_template(template, self.program_name), ack_timeout)
            self._set_state(SendState.DRIP_FEEDING if self.drip_feed else SendState.SENDING)
            tracker.start()
            if self.callbacks.on_progress:
                self.callbacks.on_progress(tracker.snapshot())
            for telegram, end in telegrams:
                link.send_telegram(telegram, ack_timeout)
                progress = tracker.advance(end - tracker.bytes_done)
                if self.callbacks.on_progress:
                    self.callbacks.on_progress(progress)
            for template in p.end_telegrams:
                link.send_telegram(_template(template, self.program_name), ack_timeout)
            self._set_state(SendState.FINISHING)
            link.finish()
            self._emit_modem()
            self._set_state(SendState.COMPLETED)
        except OperationCancelled:
            log.warning("LSV2 transfer cancelled by user after %d bytes", tracker.bytes_done)
            self._send_eot_after_cancel(link)
            self._set_state(SendState.CANCELLED)
        except Exception as exc:
            error = describe_error(exc, operation=operation, port=self.transport.name, profile_id=self.profile.id)
            self._set_state(SendState.FAILED)
        finally:
            self.transport.close()
        duration = self.clock.monotonic() - t0
        log.info(
            "LSV2 end: state=%s sent=%d/%d telegrams=%d repetitions=%d duration=%.2fs",
            self.state.value,
            tracker.bytes_done,
            len(payload.data),
            link.telegrams_sent,
            link.repetitions,
            duration,
        )
        return SendResult(
            state=self.state,
            bytes_total=len(payload.data),
            bytes_sent=tracker.bytes_done,
            started_at=started_at,
            duration_s=duration,
            sha256=sha256_bytes(payload.data),
            profile_id=self.profile.id,
            port=self.transport.name,
            simulated=self.transport.is_simulated,
            error=error,
        )

    def _ack_timeout(self) -> float | None:
        p = self.profile.protocol
        if self.drip_feed:
            return None if p.drip_feed_ack_timeout_ms == 0 else p.drip_feed_ack_timeout_ms / 1000.0
        return p.ack_timeout_ms / 1000.0

    def _emit_modem(self) -> None:
        if self.callbacks.on_modem:
            self.callbacks.on_modem(self.transport.get_modem_status())

    def _wait_lines(self) -> None:
        serial = self.profile.serial
        if not (serial.require_cts_before_send or serial.require_dsr_before_send):
            return
        deadline = self.clock.monotonic() + serial.handshake_timeout_ms / 1000.0
        while True:
            status = self.transport.get_modem_status()
            if self.callbacks.on_modem:
                self.callbacks.on_modem(status)
            missing = []
            if serial.require_cts_before_send and not status.cts:
                missing.append("CTS")
            if serial.require_dsr_before_send and not status.dsr:
                missing.append("DSR")
            if not missing:
                return
            if self.clock.monotonic() >= deadline:
                raise HandshakeTimeoutError(f"{'/'.join(missing)} nicht aktiv", self.transport.name, tuple(missing))
            if self.clock.sleep(POLL_INTERVAL_S, self.token.event):
                raise OperationCancelled("Abbruch")

    def _send_eot_after_cancel(self, link: Lsv2Link) -> None:
        if not (link.established and self.profile.protocol.send_eot_on_cancel and self.transport.is_open):
            return
        try:
            link.token = CancellationToken()  # allow this final write
            link.finish()
            log.info("LSV2: EOT sent after cancellation")
        except Exception as exc:
            log.warning("LSV2: EOT after cancellation failed: %s", exc)


class Lsv2ProgramReceiver:
    """Receives a program that the CNC sends via LSV2."""

    def __init__(
        self,
        transport: Transport,
        profile: Profile,
        *,
        clock: Clock | None = None,
        token: CancellationToken | None = None,
        callbacks: ReceiveCallbacks | None = None,
    ) -> None:
        self.transport = transport
        self.profile = profile
        self.clock = clock or SystemClock()
        self.token = token or CancellationToken()
        self.callbacks = callbacks or ReceiveCallbacks()
        self.state = ReceiveState.IDLE

    def _set_state(self, state: ReceiveState) -> None:
        self.state = state
        if self.callbacks.on_state:
            self.callbacks.on_state(state)

    def receive(self) -> ReceiveResult:
        settings = self.profile.receive
        p = self.profile.protocol
        started_at = self.clock.now()
        t0 = self.clock.monotonic()
        buffer = bytearray()
        first_at = last_at = None
        error: UserFacingError | None = None
        end_reason = EndReason.ERROR
        final_state = ReceiveState.FAILED
        ending = self.profile.transfer.line_ending.sequence
        link = Lsv2Link(self.transport, p, self.clock, self.token, self.callbacks.on_traffic)
        try:
            self._set_state(ReceiveState.OPENING_PORT)
            if not self.transport.is_open:
                self.transport.open()
            if self.callbacks.on_modem:
                self.callbacks.on_modem(self.transport.get_modem_status())
            self._set_state(ReceiveState.LISTENING)
            first_timeout = settings.first_byte_timeout_ms / 1000.0 if settings.first_byte_timeout_ms else None
            if not link.accept(first_timeout):
                end_reason = EndReason.NO_DATA
                final_state = ReceiveState.TIMED_OUT
            else:
                first_at = self.clock.now()
                self._set_state(ReceiveState.RECEIVING)
                while True:
                    event = link.receive_event(settings.inactivity_timeout_ms / 1000.0)
                    if event is None:
                        end_reason = EndReason.INACTIVITY
                        final_state = ReceiveState.TIMED_OUT
                        break
                    if event.kind is EventKind.EOT:
                        end_reason = EndReason.EOT
                        final_state = ReceiveState.COMPLETED
                        break
                    text = event.text
                    if not p.include_line_ending:
                        text += ending
                    last_at = self.clock.now()
                    if settings.max_receive_bytes and len(buffer) + len(text) > settings.max_receive_bytes:
                        end_reason = EndReason.MAX_SIZE
                        error = UserFacingError(
                            "Maximale Empfangsgröße überschritten - Empfang gestoppt.",
                            (f"Grenze laut Profil: {settings.max_receive_bytes} Bytes.",),
                            operation="Empfangen (LSV2)",
                        )
                        break
                    buffer += text
                    if self.callbacks.on_progress:
                        elapsed = self.clock.monotonic() - t0
                        self.callbacks.on_progress(ReceiveProgress(self.state, len(buffer), elapsed, 0.0))
            if end_reason is not EndReason.MAX_SIZE:
                self._set_state(ReceiveState.FINALIZING)
        except OperationCancelled:
            end_reason = EndReason.CANCELLED
            final_state = ReceiveState.CANCELLED
        except Exception as exc:
            end_reason = EndReason.ERROR
            final_state = ReceiveState.FAILED
            error = describe_error(
                exc, operation="Empfangen (LSV2)", port=self.transport.name, profile_id=self.profile.id
            )
        finally:
            self.transport.close()
        self._set_state(final_state)
        raw = bytes(buffer)
        log.info(
            "LSV2 receive end: state=%s reason=%s bytes=%d repetitions=%d",
            final_state.value,
            end_reason.value,
            len(raw),
            link.repetitions,
        )
        return ReceiveResult(
            state=final_state,
            raw=raw,
            end_reason=end_reason,
            started_at=started_at,
            duration_s=self.clock.monotonic() - t0,
            profile_id=self.profile.id,
            port=self.transport.name,
            simulated=self.transport.is_simulated,
            first_byte_at=first_at,
            last_byte_at=last_at,
            error=error,
        )
