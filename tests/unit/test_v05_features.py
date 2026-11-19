"""Format detection, commissioning record, profile exchange, comfort helpers."""

from __future__ import annotations

import json
import threading
from datetime import UTC, datetime
from pathlib import Path

import pytest

from deckellink.app import parse_args
from deckellink.clock import FakeClock
from deckellink.commissioning import (
    STEPS,
    CommissioningRecord,
    CommissioningStore,
    StepStatus,
    export_report,
    report_markdown,
)
from deckellink.config import MAX_RECENT_FILES, AppConfig, load_config, save_config
from deckellink.diagnostics.format_detect import (
    analyze_capture,
    capture_passively,
    capture_settings,
    encode_frames_as_8n1,
)
from deckellink.domain.errors import ProfileValidationError
from deckellink.domain.models import Parity, Profile, VerificationStatus
from deckellink.gui.app_support import HELP_DOCUMENTS, docs_dir, find_document
from deckellink.gui.style import build_stylesheet
from deckellink.profiles.repository import ProfileRepository, ProfileRepositoryError
from deckellink.simulation.scenarios import SAMPLE_PROGRAM
from deckellink.storage.backup import BackupManager
from deckellink.storage.paths import AppPaths
from deckellink.transfer.cancellation import CancellationToken
from deckellink.transport.simulated_transport import SimulatedTransport, SimulationConfig

NOW = datetime(2026, 11, 19, 9, 0, tzinfo=UTC)
TEXT = SAMPLE_PROGRAM * 3


# ---- passive format detection ---------------------------------------------------------
@pytest.mark.parametrize(
    ("parity", "frame"),
    [(Parity.EVEN, "7E1"), (Parity.ODD, "7O1"), (Parity.NONE, "7N1"), (Parity.SPACE, "8N1")],
)
def test_detects_frame(parity: Parity, frame: str) -> None:
    guess = analyze_capture(encode_frames_as_8n1(TEXT, parity))
    assert guess.frame == frame
    assert guess.confidence == "hoch"
    assert "Ergebnis" in guess.report()


def test_detection_rejects_too_little_and_garbage() -> None:
    assert analyze_capture(b"N10\r\n").confidence == "keine"
    garbage = bytes((i * 73 + 11) % 256 for i in range(500))
    result = analyze_capture(garbage)
    assert result.parity is None and "Baudrate" in result.report()
    mixed = bytes(b | (0x80 if i % 2 else 0) for i, b in enumerate(TEXT))
    assert analyze_capture(mixed).parity is None


def test_capture_is_receive_only(profile_a: Profile) -> None:
    clock = FakeClock()
    settings = capture_settings(profile_a.serial)
    assert (settings.data_bits, settings.parity, settings.stop_bits) == (8, Parity.NONE, 1)
    assert settings.baud_rate == profile_a.serial.baud_rate
    stream = encode_frames_as_8n1(TEXT, Parity.EVEN)
    transport = SimulatedTransport(settings, SimulationConfig(machine_program=stream), clock)
    progress: list[int] = []
    data = capture_passively(
        transport,
        duration_s=30,
        max_bytes=10_000,
        clock=clock,
        token=CancellationToken(),
        on_progress=progress.append,
    )
    assert data == stream
    assert transport.machine_received == b""  # nothing was ever written
    assert not transport.is_open
    assert progress[-1] == len(stream)
    assert analyze_capture(data).frame == "7E1"
    limited = capture_passively(
        SimulatedTransport(settings, SimulationConfig(machine_program=stream), FakeClock()),
        duration_s=30,
        max_bytes=50,
        clock=FakeClock(),
        token=CancellationToken(),
    )
    assert len(limited) == 50


# ---- commissioning --------------------------------------------------------------------
def test_commissioning_record_rules(tmp_path: Path, profile_a: Profile) -> None:
    store = CommissioningStore(tmp_path)
    record = store.load(profile_a.id, NOW)
    assert record.started_at.startswith("2026-11-19")
    assert not record.can_mark_verified()
    for step in STEPS:
        if step.required:
            record.set_status(step.id, StepStatus.DONE, NOW)
    assert not record.can_mark_verified()  # tester missing
    record.tester = "M. Muster"
    assert record.can_mark_verified()
    record.step("receive").note = "ok | 181 Bytes"
    store.save(record)
    store.save(record)
    assert store.path_for(profile_a.id).with_suffix(".json.bak").is_file()
    loaded = store.load(profile_a.id, NOW)
    assert loaded.can_mark_verified() and loaded.step("receive").note == "ok | 181 Bytes"
    report = report_markdown(loaded, profile_a, NOW)
    assert "Alle Pflichtschritte sind erledigt" in report and "ok / 181 Bytes" in report
    target = export_report(loaded, profile_a, tmp_path / "exports", NOW)
    assert target.suffix == ".md" and target.read_text(encoding="utf-8") == report
    assert export_report(loaded, profile_a, tmp_path / "exports", NOW) != target


def test_commissioning_robust_loading(tmp_path: Path) -> None:
    store = CommissioningStore(tmp_path)
    path = store.path_for("p1")
    path.parent.mkdir(parents=True)
    path.write_text("{broken", encoding="utf-8")
    assert store.load("p1", NOW).steps == {}
    path.write_text(json.dumps({"steps": {"loopback": {"status": "bogus"}, "unknown": {}}}), encoding="utf-8")
    record = store.load("p1", NOW)
    assert record.step("loopback").status is StepStatus.OPEN and "unknown" not in record.steps
    missing = CommissioningRecord("p1").missing_required()
    assert {s.id for s in missing} == {s.id for s in STEPS if s.required}


# ---- profile exchange ---------------------------------------------------------------------
def test_profile_export_import(paths: AppPaths, tmp_path: Path) -> None:
    backups = BackupManager(paths.backups)
    repo = ProfileRepository(
        Path(__file__).resolve().parents[2] / "src/deckellink/resources/profiles", paths.profiles, backups
    )
    target = tmp_path / "share" / "profil.json"
    target.parent.mkdir()
    assert repo.export_profile("dialog11_7e1_none", target) is None
    with pytest.raises(ProfileRepositoryError):
        repo.export_profile("dialog11_7e1_none", target)
    assert repo.export_profile("dialog11_7e1_none", target, overwrite=True) is not None
    with pytest.raises(ProfileRepositoryError):
        repo.export_profile("unknown", tmp_path / "x.json")
    imported, warnings = repo.import_profile(target)
    assert imported.id.startswith("dialog11_7e1_none_import") and not imported.builtin
    assert any("importiert als" in w for w in warnings)
    data = json.loads(target.read_text(encoding="utf-8"))
    data["id"] = "werkstatt_fp2a"
    data["verification_status"] = "verified"
    target.write_text(json.dumps(data), encoding="utf-8")
    verified, warnings = repo.import_profile(target)
    assert verified.id == "werkstatt_fp2a" and verified.verification_status is VerificationStatus.VERIFIED
    assert any("Herkunft" in w for w in warnings)
    target.write_text("{nope", encoding="utf-8")
    with pytest.raises(ProfileValidationError):
        repo.import_profile(target)


# ---- comfort / app support ------------------------------------------------------------------
def test_recent_files_and_font_scale(tmp_path: Path) -> None:
    config = AppConfig()
    for i in range(MAX_RECENT_FILES + 3):
        config.add_recent_file(f"/x/{i}.nc")
    config.add_recent_file("/x/5.nc")
    assert config.recent_files[0] == "/x/5.nc"
    assert len(config.recent_files) == MAX_RECENT_FILES
    assert config.recent_files.count("/x/5.nc") == 1
    config.font_scale_percent = 150
    save_config(config, tmp_path)
    loaded, warnings = load_config(tmp_path)
    assert loaded.recent_files == config.recent_files and loaded.font_scale_percent == 150 and not warnings
    (tmp_path / "settings.json").write_text(
        json.dumps({"recent_files": "nope", "font_scale_percent": 999}), encoding="utf-8"
    )
    loaded, warnings = load_config(tmp_path)
    assert loaded.recent_files == [] and loaded.font_scale_percent == 200 and warnings


def test_stylesheet_scaling() -> None:
    assert "font-size: 10.0pt" in build_stylesheet()
    big = build_stylesheet(1.5)
    assert "font-size: 15.0pt" in big and "font-size: 24.0pt" in big
    assert "{base}" not in big


def test_documentation_available() -> None:
    assert docs_dir().is_dir()
    for doc in HELP_DOCUMENTS:
        assert find_document(doc.file_name) is not None, doc.file_name


def test_cli_file_argument() -> None:
    args = parse_args(["C:/Programme/TEIL1.nc"])
    assert args.file == Path("C:/Programme/TEIL1.nc")
    assert parse_args([]).file is None


def test_fake_clock_is_thread_safe() -> None:
    clock = FakeClock()
    threads = [threading.Thread(target=clock.advance, args=(1,)) for _ in range(10)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    assert clock.monotonic() == 10
