"""Reading and writing parameter files over the configured protocol (plain or LSV2)."""

from __future__ import annotations

from deckellink.clock import Clock
from deckellink.domain.models import Profile
from deckellink.domain.results import ReceiveResult, SendResult, TransformResult
from deckellink.parameters.service import parameter_profile
from deckellink.transfer.cancellation import CancellationToken
from deckellink.transfer.lsv2_transfer import Lsv2ProgramReceiver, Lsv2ProgramSender
from deckellink.transfer.receiver import ProgramReceiver, ReceiveCallbacks
from deckellink.transfer.sender import ProgramSender, SendCallbacks
from deckellink.transport.base import Transport


def read_parameters(
    transport: Transport,
    profile: Profile,
    *,
    clock: Clock,
    token: CancellationToken,
    callbacks: ReceiveCallbacks | None = None,
) -> ReceiveResult:
    """Receive the parameter output started by the operator at the CNC."""
    effective = parameter_profile(profile)
    if effective.protocol.is_lsv2:
        return Lsv2ProgramReceiver(transport, effective, clock=clock, token=token, callbacks=callbacks).receive()
    return ProgramReceiver(transport, effective, clock=clock, token=token, callbacks=callbacks).receive()


def write_parameters(
    transport: Transport,
    profile: Profile,
    payload: TransformResult,
    *,
    clock: Clock,
    token: CancellationToken,
    callbacks: SendCallbacks | None = None,
) -> SendResult:
    """Send a parameter file into the parameter input enabled at the CNC (never drip-feed)."""
    effective = parameter_profile(profile)
    if effective.protocol.is_lsv2:
        sender = Lsv2ProgramSender(
            transport,
            effective,
            drip_feed=False,
            program_name="PARAMETER",
            clock=clock,
            token=token,
            callbacks=callbacks,
        )
        return sender.send(payload)
    return ProgramSender(transport, effective, clock=clock, token=token, callbacks=callbacks).send(payload)
