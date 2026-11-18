from __future__ import annotations

import pytest
from PySide6.QtWidgets import QLabel
from pytestqt.qtbot import QtBot

from deckellink.domain.errors import UserFacingError
from deckellink.domain.models import FALLBACK_PROFILE
from deckellink.gui.dialogs.confirm_transfer import ConfirmInfo, ConfirmTransferDialog
from deckellink.gui.dialogs.export_dialog import ExportDialog
from deckellink.gui.style import StatusKind
from deckellink.gui.widgets.error_panel import ErrorPanel
from deckellink.gui.widgets.profile_editor import FIELDS, ProfileEditor
from deckellink.gui.widgets.status import SignalPanel, StatusIndicator
from deckellink.profiles.schema import parse_profile
from deckellink.safety import SAFETY_NOTICE
from deckellink.transport.base import ModemStatus

pytestmark = pytest.mark.gui


def info(simulation: bool, notice: bool) -> ConfirmInfo:
    return ConfirmInfo(
        simulation=simulation,
        file_name="P.nc",
        file_path="/x/P.nc",
        transfer_bytes=100,
        lines=5,
        profile_name="Profil",
        profile_verified=False,
        port="COM3",
        serial_summary="4800 Baud | 7N1 | RTS/CTS",
        warnings=("Programmende fehlt",),
        applied=("Leerzeilen entfernt (1)",),
        show_safety_notice=notice,
        appended_program_end=True,
    )


def test_confirm_dialog_real_mode_shows_safety_notice(qtbot: QtBot) -> None:
    dialog = ConfirmTransferDialog(info(simulation=False, notice=True))
    qtbot.addWidget(dialog)
    assert dialog.suppress.isVisibleTo(dialog)
    all_text = " ".join(lbl.text() for lbl in dialog.findChildren(QLabel))
    assert SAFETY_NOTICE in all_text
    assert "REALE ÜBERTRAGUNG" in all_text
    dialog.suppress.setChecked(True)
    dialog.accept()
    outcome = dialog.outcome()
    assert outcome.accepted and outcome.suppress_safety_notice


def test_confirm_dialog_simulation_no_notice_and_reject(qtbot: QtBot) -> None:
    dialog = ConfirmTransferDialog(info(simulation=True, notice=True))
    qtbot.addWidget(dialog)
    assert not dialog.suppress.isVisibleTo(dialog)
    dialog.reject()
    assert not dialog.outcome().accepted


def test_error_panel(qtbot: QtBot) -> None:
    panel = ErrorPanel()
    qtbot.addWidget(panel)
    assert panel.isHidden()
    panel.show_error(UserFacingError("Titel", ("Ursache",), "Detail"))
    assert not panel.isHidden() and "Ursache" in panel.causes.text()
    assert panel.details.isHidden()
    panel.toggle.click()
    assert not panel.details.isHidden() and "▾" in panel.toggle.text()
    panel.close_button.click()
    assert panel.isHidden() and panel.error is None


def test_status_indicator_text_and_symbol(qtbot: QtBot) -> None:
    indicator = StatusIndicator()
    qtbot.addWidget(indicator)
    for kind in StatusKind:
        indicator.set_status(kind, "Text")
        assert indicator.text().startswith(kind.symbol)
        assert "Text" in indicator.text()


def test_signal_panel(qtbot: QtBot) -> None:
    panel = SignalPanel(controllable=True)
    qtbot.addWidget(panel)
    panel.set_status(ModemStatus(cts=True, dsr=False, rts=True))
    assert "EIN" in panel.text_of("CTS")
    assert "AUS" in panel.text_of("DSR")
    assert "unbekannt" in panel.text_of("RI")
    requests: list[tuple[str, bool]] = []
    panel.control_requested.connect(lambda n, v: requests.append((n, v)))
    panel.set_controls_enabled(True)
    panel._toggles["DTR"].setChecked(True)
    assert requests == [("DTR", True)]


def test_profile_editor_roundtrip(qtbot: QtBot) -> None:
    editor = ProfileEditor()
    qtbot.addWidget(editor)
    editor.load(FALLBACK_PROFILE)
    data = editor.to_dict()
    profile, _ = parse_profile(data)
    assert profile.serial == FALLBACK_PROFILE.serial
    assert profile.transfer == FALLBACK_PROFILE.transfer
    assert profile.receive == FALLBACK_PROFILE.receive
    assert len(FIELDS) >= 40
    editor.verified_box.setChecked(True)
    assert editor.to_dict()["verification_status"] == "verified"
    with pytest.raises(RuntimeError):
        ProfileEditor().to_dict()


def test_export_dialog_options(qtbot: QtBot, tmp_path: object) -> None:
    from pathlib import Path

    nc = Path(str(tmp_path)) / "A.nc"
    dialog = ExportDialog(mask_paths=True, nc_file=nc, raw_file=None)
    qtbot.addWidget(dialog)
    assert not dialog.include_raw.isEnabled()
    options = dialog.options({"state": "x"})
    assert options.include_nc_files == [] and options.mask_paths
    dialog.include_nc.setChecked(True)
    assert dialog.options(None).include_nc_files == [nc]
