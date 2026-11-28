"""Program management, tool table, line mapping and menu structure."""

from __future__ import annotations

import json
from datetime import UTC, datetime, timedelta
from pathlib import Path

import pytest

from deckellink.dialog11sim.menus import AUX_MODES, MAIN_MODES, MenuItem, build_block, structure_text
from deckellink.nc.linemap import map_payload_lines
from deckellink.storage.backup import BackupManager
from deckellink.storage.library import EntryKind, LibraryError, ProgramLibrary, guess_kind, tools_used
from deckellink.storage.tooldb import (
    Tool,
    ToolDatabase,
    ToolDataError,
    ToolExportTemplate,
    check_program_tools,
    export_tool_file,
)

T0 = datetime(2026, 11, 26, 9, 0, tzinfo=UTC)
PROGRAM = (
    b"N10 G17 T1 M6; M3\r\nN20 G0 X0 Y0 Z2\r\nN30 TT12 (T99 IM KOMMENTAR)\r\nN40 G81 OA0 TI5\r\nN50 T0\r\nN60 M30\r\n"
)


def test_tools_used_and_kind() -> None:
    assert tools_used(PROGRAM) == ["T1", "TT12"]
    assert guess_kind(b"%%4\r\nN10 G0 X0\r\n", "x.nc") is EntryKind.MACRO
    assert guess_kind(b"%MPG00 (KOPF)\r\n", "x.nc") is EntryKind.OTHER
    assert guess_kind(PROGRAM, "teil.nc") is EntryKind.PROGRAM
    assert EntryKind.TOOLS.label == "Werkzeugdatei"


def test_import_versions_restore_and_delete(tmp_path: Path) -> None:
    library = ProgramLibrary(tmp_path / "library")
    assert library.entries() == []
    source = tmp_path / "TEIL 17.nc"
    source.write_bytes(PROGRAM)
    entry = library.import_file(source, now=T0)
    assert entry.path.read_bytes() == PROGRAM and entry.tools == ["T1", "TT12"]
    second = library.import_file(source, now=T0)
    assert second.id != entry.id  # never overwritten
    assert source.read_bytes() == PROGRAM
    updated = library.save_version(entry, PROGRAM + b"N70 M30\r\n", now=T0 + timedelta(minutes=1))
    assert updated.version_count == 1 and updated.read().endswith(b"N70 M30\r\n")
    assert library.save_version(updated, updated.read(), now=T0) is updated  # unchanged -> no new version
    versions = library.versions(updated)
    assert len(versions) == 1 and versions[0].read_bytes() == PROGRAM
    restored = library.restore_version(updated, versions[0], now=T0 + timedelta(minutes=2))
    assert restored.read() == PROGRAM and restored.version_count == 2
    with pytest.raises(LibraryError):
        library.restore_version(restored, tmp_path / "fremd.nc", now=T0)
    library.update_meta(restored, name=" Flansch ", number="17", description="Test", kind="macro")
    again = library.get(restored.id)
    assert again is not None and (again.name, again.number, again.kind) == ("Flansch", "17", EntryKind.MACRO)
    with pytest.raises(LibraryError):
        library.update_meta(again, colour="rot")
    library.mark_sent(again, now=T0, mode="Simulator")
    assert library.get(again.id).last_sent_mode == "Simulator"  # type: ignore[union-attr]
    target = library.delete(again, now=T0)
    assert (target / again.path.name).is_file() and (target / "versions").is_dir()
    assert library.get(again.id) is None
    library.delete(second, now=T0)
    assert library.entries() == []


def test_library_errors_and_bad_metadata(tmp_path: Path) -> None:
    library = ProgramLibrary(tmp_path / "library")
    with pytest.raises(LibraryError):
        library.import_bytes("leer.nc", b"", now=T0)
    with pytest.raises(LibraryError):
        library.import_file(tmp_path / "missing.nc", now=T0)
    entry = library.import_bytes("A.nc", b"N10 G0 X0\r\n", kind=EntryKind.TOOLS, now=T0)
    with pytest.raises(LibraryError):
        library.save_version(entry, b"", now=T0)
    entry.meta_path.write_text("{kaputt", encoding="utf-8")
    assert library.get(entry.id).name == entry.id  # type: ignore[union-attr]
    entry.meta_path.write_text(json.dumps({"kind": "unbekannt", "tools": "x"}), encoding="utf-8")
    loaded = library.get(entry.id)
    assert loaded is not None and loaded.kind is EntryKind.OTHER and loaded.tools == []


def test_tool_database_roundtrip_and_backup(tmp_path: Path) -> None:
    backups = BackupManager(tmp_path / "backups")
    db = ToolDatabase(tmp_path / "tools.json", backups)
    assert db.load() == []
    tools = [Tool(2, "Bohrer", "Bohrer", 6.8, 102.5), Tool(1, "Fräser", diameter=8, length=75, flutes=3)]
    db.save(tools)
    assert [t.number for t in db.tools] == [1, 2] and db.get(1) is not None and db.get(9) is None
    db.save([*tools, Tool(3)])
    assert len(backups.list_backups()) == 1
    fresh = ToolDatabase(tmp_path / "tools.json")
    assert [t.number for t in fresh.load()] == [1, 2, 3] and fresh.get(2).radius == 3.4  # type: ignore[union-attr]
    for bad in ([Tool(0)], [Tool(1, diameter=-1)], [Tool(1, length=5000)], [Tool(1, feed=-1)], [Tool(1), Tool(1)]):
        with pytest.raises(ToolDataError):
            db.save(bad)
    (tmp_path / "tools.json").write_text('{"tools": [{"name": "ohne Nummer"}, {"number": 4}]}', encoding="utf-8")
    assert [t.number for t in fresh.load()] == [4] and fresh.problems
    (tmp_path / "tools.json").write_text("{", encoding="utf-8")
    assert fresh.load() == [] and fresh.problems


def test_tool_export_template_and_check() -> None:
    tools = [Tool(1, "Fraeser", diameter=8, length=75.25, flutes=3, feed=300, speed=2000)]
    with pytest.raises(ToolDataError, match="Keine Vorlage"):
        export_tool_file(tools, ToolExportTemplate())
    template = ToolExportTemplate("KOPF", "T{T} R{R} L{L} D{D} Z{Z} F{F} S{S} {NAME} {TYP}", "ENDE")
    assert template.configured
    with pytest.raises(ToolDataError, match="7-Bit"):
        export_tool_file(tools, template)  # default kind "Fräser" contains an umlaut
    ascii_tools = [Tool(1, "Fraeser", "Schaft", diameter=8, length=75.25, flutes=3, feed=300, speed=2000)]
    data = export_tool_file(ascii_tools, template)
    assert data == b"KOPF\r\nT1 R4 L75.25 D8 Z3 F300 S2000 Fraeser Schaft\r\nENDE\r\n"
    plain = export_tool_file([Tool(5, "B", "Bohrer", 0, 0)], ToolExportTemplate(line="T{T} L{L} R{R}"))
    assert plain == b"T5 L0 R0\r\n"
    with pytest.raises(ToolDataError, match="leer"):
        export_tool_file([], ToolExportTemplate(line="T{T}"))
    assert check_program_tools(["T1", "TT12"], tools) == ["TT12"]


def test_map_payload_lines() -> None:
    original = b"(KOPF)\r\n\r\nN10 G0  \r\nN20 G1 X5\r\n;KOMMENTAR\r\nN30 M30\r\n"
    payload = b"%\r\n(KOPF)\r\nN10 G0\r\nN20 G1 X5\r\nN30 M30\r\n"
    assert map_payload_lines(original, payload) == [1, 1, 3, 4, 6, 7]


def test_menu_structure() -> None:
    main2 = MAIN_MODES[2]
    slots = main2.slots()
    assert slots[0] is not None and slots[0].code == "21" and slots[2] is None
    assert main2.find("2117") is not None and main2.find("999") is None
    assert MAIN_MODES[3].find("387") is not None
    transfer = MAIN_MODES[3].find("387")
    assert transfer is not None and [s.label if s else None for s in transfer.slots()][0:3] == [
        "EINLESEN",
        "AUSLESEN",
        "NACHLADEN",
    ]
    assert set(AUX_MODES) == {"MODE", "INFO", "HELP", "EDIT", "GRAF", "DIA"}
    assert MenuItem("", "X", "x", key=3).softkey == 3 and MenuItem("40", "X", "x").softkey == 0
    assert MAIN_MODES[1].find("16") is not None
    text = structure_text()
    assert "387 DATEN-TRANSFER" in text and "nicht simuliert" in text and "abgeleitet" in text
    assert build_block("G1", {"X": "10,5", "Y": "", "F": "200"}, 30) == "N30 G1 X10.5 F200"
    assert build_block("MILL", {"G": "87.2", "D": "30"}, None) == "G87.2 D30"
