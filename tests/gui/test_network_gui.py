from __future__ import annotations

import shutil
from pathlib import Path

import pytest
from pytestqt.qtbot import QtBot

from deckellink.config import AppConfig
from deckellink.domain.states import SendState
from deckellink.gui.style import StatusKind
from tests.gui.conftest import WindowFactory, wait_idle
from tests.unit.test_network_transport import TcpDeviceServer

pytestmark = pytest.mark.gui


def test_network_settings_and_gate(make_window: WindowFactory) -> None:
    window = make_window(AppConfig(simulation_mode=False))
    page = window.settings_page
    page.kind_combo.setCurrentIndex(page.kind_combo.findData("tcp"))
    assert window.config.connection_kind == "tcp"
    assert not page.port_combo.isEnabled() and page.host_edit.isEnabled()
    assert "TCP-Rohdaten" in page.network_hint.text()
    assert window.header.status.kind is StatusKind.IDLE
    assert not window.receive_page.start_button.isEnabled()
    window.set_network_connection("tcp", "192.168.0.50", 4001)
    assert window.port_label == "TCP 192.168.0.50:4001"
    assert "TCP 192.168.0.50:4001" in window.header.mode_banner.text()
    assert window.receive_page.start_button.isEnabled()


def test_send_through_tcp_adapter(qtbot: QtBot, make_window: WindowFactory, fixtures_dir: Path, tmp_path: Path) -> None:
    server = TcpDeviceServer()
    try:
        window = make_window(AppConfig(simulation_mode=False, active_profile_id="dialog11_7e1_none"))
        window.set_network_connection("tcp", "127.0.0.1", server.port)
        target = tmp_path / "valid.nc"
        shutil.copy(fixtures_dir / "valid.nc", target)
        window.load_file(target)
        window.start_send()
        wait_idle(qtbot, window, timeout=20000)
        assert window.transfer_page.state.status_text == SendState.COMPLETED.label
        assert window.confirmations[0].port == f"TCP 127.0.0.1:{server.port}"
        assert server.done.wait(5)
        assert bytes(server.received) == target.read_bytes()
    finally:
        server.close()
