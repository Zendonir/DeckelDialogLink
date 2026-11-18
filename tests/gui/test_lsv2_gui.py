from __future__ import annotations

import shutil
from pathlib import Path

import pytest
from PySide6.QtWidgets import QLabel
from pytestqt.qtbot import QtBot

from deckellink.config import AppConfig
from deckellink.domain.states import ReceiveState, SendState
from deckellink.gui.dialogs.confirm_transfer import ConfirmInfo, ConfirmTransferDialog
from deckellink.gui.style import StatusKind
from deckellink.power import keep_system_awake
from deckellink.safety import DRIP_FEED_NOTICE
from deckellink.storage.paths import AppPaths
from tests.gui.conftest import WindowFactory, wait_idle

pytestmark = pytest.mark.gui

LSV2 = "dialog11_lsv2_7e1"


def load(window: object, fixtures_dir: Path, tmp_path: Path) -> None:
    target = tmp_path / "valid.nc"
    shutil.copy(fixtures_dir / "valid.nc", target)
    window.load_file(target)  # type: ignore[attr-defined]


def test_dripfeed_button_only_with_lsv2(make_window: WindowFactory, fixtures_dir: Path, tmp_path: Path) -> None:
    window = make_window()
    load(window, fixtures_dir, tmp_path)
    assert window.transfer_page.dripfeed_button.isHidden()
    window.select_profile(LSV2)
    assert not window.transfer_page.dripfeed_button.isHidden()
    assert window.transfer_page.dripfeed_button.isEnabled()
    assert "LSV2" in window.header.connection_label.text()
    window.select_profile("dialog11_7n1_rtscts")
    window.start_drip_feed()
    assert not window.busy


def test_dripfeed_in_simulator(qtbot: QtBot, make_window: WindowFactory, fixtures_dir: Path, tmp_path: Path) -> None:
    window = make_window(AppConfig(active_profile_id=LSV2, simulation_scenario="lsv2_dripfeed"), scale=50)
    load(window, fixtures_dir, tmp_path)
    states: list[str] = []
    window.transfer_page.state.setObjectName("state")
    window.transfer_page.dripfeed_button.click()
    qtbot.waitUntil(
        lambda: window.transfer_page.state.status_text == SendState.DRIP_FEEDING.label or not window.busy, timeout=10000
    )
    states.append(window.transfer_page.state.status_text)
    if window.busy:
        assert window.header.status.kind is StatusKind.ACTIVE
        assert "Nachladebetrieb" in window.header.status.status_text
    wait_idle(qtbot, window, timeout=30000)
    assert window.transfer_page.state.status_text == SendState.COMPLETED.label
    info = window.confirmations[0]
    assert info.drip_feed and "LSV2" in info.protocol
    assert "<ENQ>" in window.terminal_page.view.toPlainText() or "05" in window.terminal_page.view.toPlainText()


def test_lsv2_send_error_shown(qtbot: QtBot, make_window: WindowFactory, fixtures_dir: Path, tmp_path: Path) -> None:
    window = make_window(AppConfig(active_profile_id=LSV2, simulation_scenario="lsv2_cnc_abort"))
    load(window, fixtures_dir, tmp_path)
    window.start_send()
    wait_idle(qtbot, window)
    assert "EOT" in window.transfer_page.error_panel.title.text()


def test_lsv2_receive_in_simulator(qtbot: QtBot, make_window: WindowFactory, paths: AppPaths) -> None:
    window = make_window(AppConfig(active_profile_id=LSV2, simulation_scenario="lsv2_nak"))
    window.start_receive()
    wait_idle(qtbot, window)
    assert window.receive_page.state.status_text == ReceiveState.COMPLETED.label
    assert window.receive_page.end_reason.text() == "Übertragungsende (EOT) der CNC"
    assert list(paths.received.glob("*.nc"))


def test_confirm_dialog_requires_dripfeed_acknowledgement(qtbot: QtBot) -> None:
    info = ConfirmInfo(
        simulation=False,
        file_name="P.nc",
        file_path="/P.nc",
        transfer_bytes=10,
        lines=2,
        profile_name="LSV2",
        profile_verified=False,
        port="COM3",
        serial_summary="4800 Baud | 7E1",
        protocol="LSV2",
        drip_feed=True,
    )
    dialog = ConfirmTransferDialog(info)
    qtbot.addWidget(dialog)
    assert not dialog.start_button.isEnabled()
    assert dialog.start_button.text() == "Nachladebetrieb starten"
    assert any(label.text() == DRIP_FEED_NOTICE for label in dialog.findChildren(QLabel))
    dialog.drip_ack.setChecked(True)
    assert dialog.start_button.isEnabled()


def test_profile_editor_protocol_tab(make_window: WindowFactory) -> None:
    window = make_window()
    editor = window.settings_page.editor
    window.select_profile(LSV2)
    data = editor.to_dict()
    assert data["protocol"]["mode"] == "lsv2"
    assert data["protocol"]["start_telegrams"] == []
    editor.widget("protocol", "start_telegrams").setText("%{name} | M82")  # type: ignore[attr-defined]
    data = editor.to_dict()
    assert data["protocol"]["start_telegrams"] == ["%{name}", "M82"]
    window.save_profile(data)
    assert window.profile.protocol.start_telegrams == ("%{name}", "M82")
    assert window.profile.id != LSV2


def test_keep_awake_is_safe_everywhere() -> None:
    with keep_system_awake(True) as active:
        assert isinstance(active, bool)
    with keep_system_awake(False) as active:
        assert active is False
