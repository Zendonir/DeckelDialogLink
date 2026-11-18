"""Headless pipeline runner for simulator scenarios and the smoke test.

Uses exactly the same analyzer, transformer, sender, receiver and archive as
the GUI - only the transport is simulated.
"""

from __future__ import annotations

import json
import logging
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from deckellink.clock import Clock, FakeClock
from deckellink.domain.models import Profile
from deckellink.domain.results import ReceiveResult, SendResult, TransferProgress
from deckellink.domain.states import ReceiveState, SendState
from deckellink.nc.analyzer import analyze
from deckellink.nc.transformer import transform
from deckellink.profiles.loader import builtin_profiles_dir
from deckellink.profiles.repository import ProfileRepository
from deckellink.simulation.lsv2_cnc import Lsv2SimulatedCnc
from deckellink.simulation.scenarios import SAMPLE_PROGRAM, Scenario, get_scenario, lsv2_config_for
from deckellink.storage.archive import ReceiveArchive
from deckellink.storage.backup import BackupManager
from deckellink.storage.checksums import sha256_bytes, sha256_file
from deckellink.storage.files import unique_path, write_new_file
from deckellink.storage.paths import AppPaths
from deckellink.transfer.cancellation import CancellationToken
from deckellink.transfer.lsv2_transfer import Lsv2ProgramReceiver, Lsv2ProgramSender
from deckellink.transfer.receiver import ProgramReceiver, ReceiveCallbacks, process_received
from deckellink.transfer.sender import ProgramSender, SendCallbacks
from deckellink.transport.simulated_transport import SimulatedTransport

log = logging.getLogger(__name__)


def run_send_scenario(
    scenario: Scenario,
    profile: Profile,
    *,
    clock: Clock | None = None,
    data: bytes | None = None,
) -> tuple[SendResult, SimulatedTransport]:
    clock = clock or FakeClock()
    transport = SimulatedTransport(profile.serial, scenario.config, clock)
    token = CancellationToken()
    payload = transform(data if data is not None else scenario.sample_program, profile.transfer)

    def on_progress(progress: TransferProgress) -> None:
        limit = scenario.auto_cancel_after_bytes
        if limit is not None and progress.bytes_done >= limit:
            token.cancel()

    sender = ProgramSender(
        transport, profile, clock=clock, token=token, callbacks=SendCallbacks(on_progress=on_progress)
    )
    return sender.send(payload), transport


def run_receive_scenario(
    scenario: Scenario,
    profile: Profile,
    *,
    clock: Clock | None = None,
) -> ReceiveResult:
    clock = clock or FakeClock()
    transport = SimulatedTransport(profile.serial, scenario.config, clock)
    token = CancellationToken()
    received = 0

    def on_traffic(event: Any) -> None:
        nonlocal received
        received += len(event.data)
        limit = scenario.auto_cancel_after_bytes
        if limit is not None and received >= limit:
            token.cancel()

    receiver = ProgramReceiver(
        transport, profile, clock=clock, token=token, callbacks=ReceiveCallbacks(on_traffic=on_traffic)
    )
    return receiver.receive()


def run_lsv2_send_scenario(
    scenario: Scenario,
    profile: Profile,
    *,
    drip_feed: bool = False,
    clock: Clock | None = None,
    data: bytes | None = None,
    cancel_after_bytes: int | None = None,
) -> tuple[SendResult, Lsv2SimulatedCnc]:
    clock = clock or FakeClock()
    cnc = Lsv2SimulatedCnc(profile.serial, profile.protocol, lsv2_config_for(scenario, drip_feed=drip_feed), clock)
    token = CancellationToken()
    payload = transform(data if data is not None else scenario.sample_program, profile.transfer)

    def on_progress(progress: TransferProgress) -> None:
        if cancel_after_bytes is not None and progress.bytes_done >= cancel_after_bytes:
            token.cancel()

    sender = Lsv2ProgramSender(
        cnc,
        profile,
        drip_feed=drip_feed,
        clock=clock,
        token=token,
        callbacks=SendCallbacks(on_progress=on_progress),
    )
    return sender.send(payload), cnc


def run_lsv2_receive_scenario(
    scenario: Scenario, profile: Profile, *, clock: Clock | None = None
) -> tuple[ReceiveResult, Lsv2SimulatedCnc]:
    clock = clock or FakeClock()
    cnc = Lsv2SimulatedCnc(profile.serial, profile.protocol, lsv2_config_for(scenario, receive=True), clock)
    return Lsv2ProgramReceiver(cnc, profile, clock=clock).receive(), cnc


@dataclass
class SmokeReport:
    steps: list[dict[str, Any]] = field(default_factory=list)
    ok: bool = True

    def step(self, name: str, ok: bool, **details: Any) -> None:
        self.steps.append({"step": name, "ok": ok, **details})
        if not ok:
            self.ok = False

    def to_json(self) -> str:
        return json.dumps({"ok": self.ok, "steps": self.steps}, indent=2, ensure_ascii=False)


def run_smoke_test(paths: AppPaths, report_path: Path | None = None) -> SmokeReport:
    """End-to-end pipeline in simulation mode without GUI."""
    report = SmokeReport()
    try:
        paths.ensure()
        repo = ProfileRepository(builtin_profiles_dir(), paths.profiles)
        profiles = [p.id for p in repo.profiles]
        report.step("profiles_loaded", len(profiles) >= 2 and not repo.problems, profiles=profiles)
        profile = repo.default_profile()

        sample = unique_path(paths.simulation, "SMOKE_TEST", ".nc")
        write_new_file(sample, SAMPLE_PROGRAM)
        original = sample.read_bytes()
        analysis = analyze(original, profile.transfer, profile.serial)
        report.step(
            "validated",
            not analysis.has_errors,
            findings=[f.code for f in analysis.findings],
        )
        payload = transform(original, profile.transfer)
        send_result, transport = run_send_scenario(get_scenario("send_ok"), profile, data=original)
        report.step(
            "simulated_send",
            send_result.state is SendState.COMPLETED and transport.machine_received == payload.data,
            bytes=send_result.bytes_sent,
        )
        receive_result = run_receive_scenario(get_scenario("receive_ok"), profile)
        program = process_received(receive_result, profile)
        report.step(
            "simulated_receive",
            receive_result.state is ReceiveState.COMPLETED and program.complete,
            bytes=len(receive_result.raw),
            end_reason=receive_result.end_reason.value,
        )
        archive = ReceiveArchive(paths)
        stored = archive.store(receive_result, program, profile)
        ok_store = stored is not None and stored.nc_path.is_file()
        digest = sha256_file(stored.nc_path) if stored else ""
        report.step(
            "stored",
            ok_store and digest == sha256_bytes(program.normalized),
            file=stored.nc_path.name if stored else None,
            sha256=digest,
        )
        report.step("original_unchanged", sample.read_bytes() == original)
        lsv2_profile = repo.get("dialog11_lsv2_7e1")
        if lsv2_profile is None:
            report.step("lsv2_dripfeed", False, error="LSV2-Profil fehlt")
        else:
            drip, cnc = run_lsv2_send_scenario(
                get_scenario("lsv2_dripfeed"), lsv2_profile, drip_feed=True, data=original
            )
            report.step(
                "lsv2_dripfeed",
                drip.state is SendState.COMPLETED and cnc.received_program == payload.data and cnc.eot_received,
                telegrams=cnc.telegrams_received,
            )
            lsv2_rx, _ = run_lsv2_receive_scenario(get_scenario("lsv2_ok"), lsv2_profile)
            report.step(
                "lsv2_receive",
                lsv2_rx.state is ReceiveState.COMPLETED and lsv2_rx.raw == SAMPLE_PROGRAM,
                bytes=len(lsv2_rx.raw),
            )
        BackupManager(paths.backups).backup(sample)
        report.step("backup", any(paths.backups.rglob("SMOKE_TEST*")))
    except Exception as exc:  # report every failure instead of crashing
        log.exception("Smoke test failed")
        report.step("exception", False, error=f"{type(exc).__name__}: {exc}")
    if report_path is not None:
        report_path.parent.mkdir(parents=True, exist_ok=True)
        report_path.write_text(report.to_json(), encoding="utf-8")
    return report
