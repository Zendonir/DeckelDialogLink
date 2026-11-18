from __future__ import annotations

import json
import os
from dataclasses import replace
from datetime import UTC, datetime
from pathlib import Path

import pytest

from deckellink.clock import FakeClock
from deckellink.domain.models import Profile
from deckellink.domain.results import ReceiveResult
from deckellink.domain.states import EndReason, ReceiveState
from deckellink.simulation.scenarios import SAMPLE_PROGRAM
from deckellink.storage.archive import ReceiveArchive, derive_stem, export_copy
from deckellink.storage.backup import BackupManager
from deckellink.storage.checksums import sha256_bytes, sha256_file
from deckellink.storage.files import (
    FileExistsConflict,
    atomic_write_bytes,
    sanitize_stem,
    unique_path,
    write_new_file,
)
from deckellink.storage.paths import ENV_HOME, AppPaths
from deckellink.transfer.receiver import process_received

WHEN = datetime(2026, 11, 18, 9, 45, 22, tzinfo=UTC)


def make_result(
    raw: bytes, state: ReceiveState = ReceiveState.COMPLETED, reason: EndReason = EndReason.END_SEQUENCE
) -> ReceiveResult:
    return ReceiveResult(
        state=state,
        raw=raw,
        end_reason=reason,
        started_at=WHEN,
        duration_s=1.5,
        profile_id="dialog11_7n1_rtscts",
        port="SIMULATOR",
        simulated=True,
        first_byte_at=WHEN,
        last_byte_at=WHEN,
    )


def test_unique_names_deterministic(tmp_path: Path) -> None:
    assert unique_path(tmp_path, "PROGRAMM", ".nc").name == "PROGRAMM.nc"
    (tmp_path / "PROGRAMM.nc").write_bytes(b"1")
    assert unique_path(tmp_path, "PROGRAMM", ".nc").name == "PROGRAMM_001.nc"
    (tmp_path / "PROGRAMM_001.nc").write_bytes(b"2")
    assert unique_path(tmp_path, "PROGRAMM", ".nc").name == "PROGRAMM_002.nc"


def test_write_new_file_never_overwrites(tmp_path: Path) -> None:
    target = tmp_path / "a.nc"
    write_new_file(target, b"first")
    with pytest.raises(FileExistsConflict):
        write_new_file(target, b"second")
    assert target.read_bytes() == b"first"


def test_atomic_write(tmp_path: Path) -> None:
    target = tmp_path / "x.json"
    atomic_write_bytes(target, b"1")
    with pytest.raises(FileExistsConflict):
        atomic_write_bytes(target, b"2")
    atomic_write_bytes(target, b"3", overwrite=True)
    assert target.read_bytes() == b"3"
    assert [p.name for p in tmp_path.iterdir()] == ["x.json"]


def test_sanitize_stem() -> None:
    assert sanitize_stem("O1234 Teil/A") == "O1234_Teil_A"
    assert sanitize_stem("///") == "PROGRAMM"


def test_checksums(tmp_path: Path) -> None:
    path = tmp_path / "f"
    path.write_bytes(b"abc")
    expected = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    assert sha256_bytes(b"abc") == expected
    assert sha256_file(path) == expected


def test_backup_naming_and_no_silent_delete(tmp_path: Path) -> None:
    clock = FakeClock(WHEN)
    manager = BackupManager(tmp_path / "backups", clock)
    source = tmp_path / "PROGRAMM.nc"
    source.write_bytes(b"data")
    first = manager.backup(source)
    second = manager.backup(source)
    assert first.parent.name == "2026-11-18"
    assert first.name == "PROGRAMM_09-45-22.nc"
    assert second.name == "PROGRAMM_09-45-22_001.nc"
    assert manager.expired_backups() == []
    assert manager.apply_retention() == []
    assert len(manager.list_backups()) == 2


def test_backup_retention_only_when_enabled(tmp_path: Path) -> None:
    clock = FakeClock(WHEN)
    manager = BackupManager(tmp_path / "backups", clock, retention_days=30)
    old_dir = tmp_path / "backups" / "2026-01-01"
    old_dir.mkdir(parents=True)
    (old_dir / "OLD.nc").write_bytes(b"x")
    (tmp_path / "backups" / "not-a-date").mkdir()
    source = tmp_path / "NEW.nc"
    source.write_bytes(b"y")
    manager.backup(source)
    deleted = manager.apply_retention()
    assert [p.name for p in deleted] == ["OLD.nc"]
    assert not old_dir.exists()
    assert len(manager.list_backups()) == 1


def test_derive_stem() -> None:
    settings = Profile(id="x", display_name="x").receive
    assert derive_stem(b"%\r\nO1234\r\n", settings, WHEN) == "RECEIVED_20261118_094522"
    with_pattern = replace(settings, program_name_pattern=r"\b(O\d{1,5})\b")
    assert derive_stem(b"%\r\nO1234\r\n", with_pattern, WHEN) == "O1234"


def test_archive_store_complete(paths: AppPaths, profile_a: Profile) -> None:
    raw = b"\x00\x00" + SAMPLE_PROGRAM
    result = make_result(raw)
    program = process_received(result, profile_a)
    archive = ReceiveArchive(paths)
    stored = archive.store(result, program, profile_a)
    assert stored is not None
    assert stored.complete
    assert stored.nc_path.parent == paths.received
    assert stored.nc_path.name == "RECEIVED_20261118_094522.nc"
    assert stored.nc_path.read_bytes() == SAMPLE_PROGRAM
    assert stored.raw_path is not None and stored.raw_path.read_bytes() == raw
    meta = json.loads(stored.meta_path.read_text(encoding="utf-8"))
    assert meta["sha256_raw"] == sha256_bytes(raw)
    assert meta["sha256_normalized"] == sha256_file(stored.nc_path)
    assert meta["raw_bytes"] == len(raw)
    assert meta["normalized_bytes"] == len(SAMPLE_PROGRAM)
    assert meta["end_reason"] == "end_sequence"
    assert meta["profile_id"] == "dialog11_7n1_rtscts"
    assert meta["port"] == "SIMULATOR"
    assert meta["serial"]["baud_rate"] == 4800
    assert meta["serial"]["parity"] == "none"
    assert meta["received_at"].startswith("2026-11-18T09:45:22")
    # Second receive with same timestamp never overwrites
    stored2 = archive.store(result, program, profile_a)
    assert stored2 is not None
    assert stored2.nc_path.name == "RECEIVED_20261118_094522_001.nc"
    assert stored.nc_path.read_bytes() == SAMPLE_PROGRAM
    listed = archive.list_programs()
    assert {p.path.name for p in listed} == {stored.nc_path.name, stored2.nc_path.name}
    assert all(p.complete for p in listed)


def test_archive_incomplete_goes_to_failed(paths: AppPaths, profile_a: Profile) -> None:
    data = SAMPLE_PROGRAM[:50]
    result = make_result(data, ReceiveState.CANCELLED, EndReason.CANCELLED)
    program = process_received(result, profile_a)
    stored = ReceiveArchive(paths).store(result, program, profile_a)
    assert stored is not None and not stored.complete
    assert stored.nc_path.parent == paths.failed
    (paths.failed / "broken.nc").write_bytes(b"x")
    (paths.failed / "broken.json").write_text("{nope", encoding="utf-8")
    listed = {p.path.name: p for p in ReceiveArchive(paths).list_programs()}
    assert listed[stored.nc_path.name].complete is False
    assert listed["broken.nc"].complete is None


def test_archive_nothing_received(paths: AppPaths, profile_a: Profile) -> None:
    result = make_result(b"", ReceiveState.TIMED_OUT, EndReason.NO_DATA)
    assert ReceiveArchive(paths).store(result, process_received(result, profile_a), profile_a) is None


def test_archive_without_raw(paths: AppPaths, profile_a: Profile) -> None:
    profile = replace(profile_a, receive=replace(profile_a.receive, save_raw=False))
    result = make_result(SAMPLE_PROGRAM)
    stored = ReceiveArchive(paths).store(result, process_received(result, profile), profile)
    assert stored is not None and stored.raw_path is None


def test_export_copy_with_backup(tmp_path: Path) -> None:
    clock = FakeClock(WHEN)
    backups = BackupManager(tmp_path / "backups", clock)
    source = tmp_path / "src.nc"
    source.write_bytes(b"NEW")
    target = tmp_path / "out" / "PROG.nc"
    assert export_copy(source, target, backups, overwrite=False) is None
    source.write_bytes(b"NEWER")
    with pytest.raises(FileExistsConflict):
        export_copy(source, target, backups, overwrite=False)
    backup = export_copy(source, target, backups, overwrite=True)
    assert backup is not None and backup.read_bytes() == b"NEW"
    assert target.read_bytes() == b"NEWER"
    with pytest.raises(FileExistsConflict):
        export_copy(source, source, backups, overwrite=True)


def test_paths_resolution(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    explicit = AppPaths.default(tmp_path / "x")
    assert explicit.received == tmp_path / "x" / "received"
    monkeypatch.setenv(ENV_HOME, str(tmp_path / "env"))
    assert AppPaths.default().root == tmp_path / "env"
    monkeypatch.delenv(ENV_HOME)
    default = AppPaths.default()
    assert "DeckelLink" in str(default.root)
    created = AppPaths.from_root(tmp_path / "y").ensure()
    for attr in ("config", "logs", "received", "backups", "failed", "raw", "exports", "profiles", "simulation"):
        assert getattr(created, attr).is_dir()
    assert os.access(created.root, os.W_OK)
