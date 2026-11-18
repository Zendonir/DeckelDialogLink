from __future__ import annotations

import json
from dataclasses import replace
from pathlib import Path
from typing import Any

import pytest

from deckellink.domain.errors import ProfileValidationError
from deckellink.domain.models import FALLBACK_PROFILE, Parity, Profile, VerificationStatus
from deckellink.profiles.loader import builtin_profiles_dir, load_profile_file
from deckellink.profiles.repository import ProfileRepository, ProfileRepositoryError
from deckellink.profiles.schema import parse_profile, safety_warnings
from deckellink.storage.backup import BackupManager


def builtin_data(name: str = "dialog11_7n1_rtscts") -> dict[str, Any]:
    data: dict[str, Any] = json.loads((builtin_profiles_dir() / f"{name}.json").read_text(encoding="utf-8"))
    return data


def test_both_builtin_profiles_load(repo: ProfileRepository) -> None:
    assert repo.problems == []
    a = repo.get("dialog11_7n1_rtscts")
    b = repo.get("dialog11_7e1_none")
    assert a is not None and b is not None
    assert a.builtin and b.builtin
    assert (a.serial.baud_rate, a.serial.frame, a.serial.rts_cts, a.serial.require_cts_before_send) == (
        4800,
        "7N1",
        True,
        True,
    )
    assert (b.serial.baud_rate, b.serial.frame, b.serial.rts_cts, b.serial.require_cts_before_send) == (
        4800,
        "7E1",
        False,
        False,
    )
    assert not b.serial.xon_xoff and not b.serial.dsr_dtr
    assert a.verification_status is VerificationStatus.UNVERIFIED
    assert b.verification_status is VerificationStatus.UNVERIFIED
    assert a.transfer.program_end == "M30" and a.transfer.program_start == ""
    assert not a.transfer.append_program_end


def test_profile_roundtrip() -> None:
    profile, warnings = parse_profile(builtin_data())
    again, _ = parse_profile(profile.to_dict())
    assert again == profile
    assert not any("fehlt" in w for w in warnings)


@pytest.mark.parametrize(
    ("section", "key", "value", "field"),
    [
        ("serial", "parity", "evenish", "serial.parity"),
        ("serial", "baud_rate", 4801, "serial.baud_rate"),
        ("serial", "data_bits", 9, "serial.data_bits"),
        ("serial", "stop_bits", 3, "serial.stop_bits"),
        ("serial", "rts_cts", "yes", "serial.rts_cts"),
        ("serial", "read_timeout_ms", 0, "serial.read_timeout_ms"),
        ("transfer", "encoding", "utf-8", "transfer.encoding"),
        ("transfer", "program_end", "M30ä", "transfer.program_end"),
        ("transfer", "start_delay_ms", True, "transfer.start_delay_ms"),
    ],
)
def test_invalid_fields_rejected(section: str, key: str, value: Any, field: str) -> None:
    data = builtin_data()
    data[section][key] = value
    with pytest.raises(ProfileValidationError) as info:
        parse_profile(data)
    assert field in [e.field for e in info.value.errors]


def test_multiple_errors_are_all_reported() -> None:
    data = builtin_data()
    data["serial"]["parity"] = "x"
    data["serial"]["baud_rate"] = 1
    data["id"] = "Invalid ID!"
    with pytest.raises(ProfileValidationError) as info:
        parse_profile(data)
    fields = {e.field for e in info.value.errors}
    assert {"serial.parity", "serial.baud_rate", "id"} <= fields


def test_missing_section_and_schema_version() -> None:
    data = builtin_data()
    del data["serial"]
    data["schema_version"] = 2
    with pytest.raises(ProfileValidationError) as info:
        parse_profile(data)
    fields = {e.field for e in info.value.errors}
    assert {"serial", "schema_version"} <= fields
    with pytest.raises(ProfileValidationError):
        parse_profile(["not", "a", "dict"])


def test_missing_field_uses_default_with_warning_and_unknown_field_warns() -> None:
    data = builtin_data()
    del data["serial"]["inter_byte_timeout_ms"]
    data["serial"]["magic"] = 1
    data["extra"] = True
    profile, warnings = parse_profile(data)
    assert profile.serial.inter_byte_timeout_ms == 0
    assert any("inter_byte_timeout_ms" in w for w in warnings)
    assert any("serial.magic" in w for w in warnings)
    assert any("'extra'" in w for w in warnings)


def test_receive_section_optional() -> None:
    data = builtin_data()
    del data["receive"]
    profile, _ = parse_profile(data)
    assert profile.receive.end_sequence == "M30"


def test_receive_semantics() -> None:
    data = builtin_data()
    data["receive"]["end_sequence"] = ""
    with pytest.raises(ProfileValidationError):
        parse_profile(data)
    data = builtin_data()
    data["receive"]["program_name_pattern"] = "(unclosed"
    with pytest.raises(ProfileValidationError):
        parse_profile(data)
    data["receive"]["program_name_pattern"] = "O\\d+"
    with pytest.raises(ProfileValidationError):
        parse_profile(data)


def test_safety_warnings() -> None:
    profile, _ = parse_profile(builtin_data())
    assert safety_warnings(profile) == []
    risky = replace(
        profile,
        serial=replace(profile.serial, data_bits=6, initial_rts=False, read_timeout_ms=10, xon_xoff=True),
        transfer=replace(profile.transfer, append_program_end=True, program_start="M82"),
        receive=replace(profile.receive, max_receive_bytes=0),
    )
    text = "\n".join(safety_warnings(risky))
    for expected in (
        "6 Datenbits",
        "RTS manuell",
        "Mehrere Flusssteuerungen",
        "Unbegrenzter Empfang",
        "Extrem kurze Timeouts",
        "Ergänzen von 'M30'",
        "M82",
    ):
        assert expected in text


def test_malformed_json_handled(tmp_path: Path) -> None:
    bad = tmp_path / "bad.json"
    bad.write_text("{ not json", encoding="utf-8")
    result = load_profile_file(bad)
    assert not result.ok
    assert result.error is not None and "Ungültiges JSON" in result.error
    missing = load_profile_file(tmp_path / "missing.json")
    assert not missing.ok


def test_invalid_user_profile_does_not_crash_and_is_reported(paths: Any) -> None:
    (paths.profiles / "broken.json").write_text('{"schema_version": 1, "id": "broken"}', encoding="utf-8")
    repo = ProfileRepository(builtin_profiles_dir(), paths.profiles)
    assert len(repo.problems) == 1
    assert "serial" in (repo.problems[0].error or "")
    assert len(repo.profiles) == 2


def test_fallback_to_safe_default(tmp_path: Path) -> None:
    repo = ProfileRepository(tmp_path / "no_builtin", tmp_path / "user")
    profile, fell_back = repo.resolve("does_not_exist")
    assert fell_back
    assert profile == FALLBACK_PROFILE
    real = ProfileRepository(builtin_profiles_dir(), tmp_path / "user")
    profile, fell_back = real.resolve("does_not_exist")
    assert fell_back and profile.id == "dialog11_7n1_rtscts"
    profile, fell_back = real.resolve(None)
    assert not fell_back


def test_builtin_profile_never_overwritten(paths: Any) -> None:
    repo = ProfileRepository(builtin_profiles_dir(), paths.profiles, BackupManager(paths.backups))
    builtin_file = builtin_profiles_dir() / "dialog11_7n1_rtscts.json"
    before = builtin_file.read_bytes()
    original = repo.get("dialog11_7n1_rtscts")
    assert original is not None
    edited = replace(original, serial=replace(original.serial, baud_rate=9600, parity=Parity.EVEN))
    stored, _ = repo.save(edited)
    assert builtin_file.read_bytes() == before
    assert stored.id != original.id
    assert stored.id.startswith("dialog11_7n1_rtscts_benutzer")
    assert "(angepasst)" in stored.display_name
    assert not stored.builtin
    assert (paths.profiles / f"{stored.id}.json").is_file()
    assert repo.get("dialog11_7n1_rtscts") == original
    # reload from disk finds the copy
    repo2 = ProfileRepository(builtin_profiles_dir(), paths.profiles)
    copy = repo2.get(stored.id)
    assert copy is not None and copy.serial.baud_rate == 9600


def test_user_profile_update_creates_backup_and_delete(paths: Any) -> None:
    backups = BackupManager(paths.backups)
    repo = ProfileRepository(builtin_profiles_dir(), paths.profiles, backups)
    copy, _ = repo.duplicate("dialog11_7e1_none", "Meine Kopie")
    assert copy.display_name == "Meine Kopie"
    updated, _ = repo.save(replace(copy, description="geändert"))
    assert updated.id == copy.id
    assert len(backups.list_backups()) == 1
    with pytest.raises(ProfileRepositoryError):
        repo.delete("dialog11_7e1_none")
    repo.delete(copy.id)
    assert repo.get(copy.id) is None
    assert len(backups.list_backups()) == 2
    with pytest.raises(ProfileRepositoryError):
        repo.delete("unknown")
    with pytest.raises(ProfileRepositoryError):
        repo.duplicate("unknown")


def test_duplicate_id_in_user_dir_ignored(paths: Any) -> None:
    data = builtin_data()
    (paths.profiles / "dup.json").write_text(json.dumps(data), encoding="utf-8")
    repo = ProfileRepository(builtin_profiles_dir(), paths.profiles)
    assert any("bereits vergeben" in (p.error or "") for p in repo.problems)
    assert repo.is_builtin("dialog11_7n1_rtscts")


def test_profile_properties() -> None:
    profile: Profile = FALLBACK_PROFILE
    assert profile.serial.summary == "4800 Baud | 7N1 | RTS/CTS"
    assert profile.serial.bits_per_character == 9  # 1 start + 7 data + 0 parity + 1 stop
    assert profile.serial.bytes_per_second == pytest.approx(4800 / 9)
    assert replace(profile.serial, parity=Parity.EVEN).bits_per_character == 10
    assert replace(profile.serial, stop_bits=1.5).frame == "7N1.5"
    assert replace(profile.serial, rts_cts=False).flow_control_label == "Kein Handshake"
