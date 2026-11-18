"""GUI smoke test (headless, Qt offscreen platform).

1. start application in simulator  2. load valid NC file  3. validate
4. simulated send  5. simulated receive  6. store received file
7. SHA-256  8. close without unhandled exception
"""

from __future__ import annotations

import shutil
import sys
from pathlib import Path

import pytest
from pytestqt.qtbot import QtBot

from deckellink.config import AppConfig
from deckellink.gui.dialogs.confirm_transfer import ConfirmOutcome
from deckellink.gui.main_window import MainWindow
from deckellink.storage.checksums import sha256_file
from deckellink.storage.paths import AppPaths

pytestmark = pytest.mark.gui


def test_gui_smoke(qtbot: QtBot, paths: AppPaths, fixtures_dir: Path, tmp_path: Path) -> None:
    unhandled: list[BaseException] = []
    previous_hook = sys.excepthook
    sys.excepthook = lambda _t, exc, _tb: unhandled.append(exc)
    try:
        window = MainWindow(
            paths, AppConfig(simulation_mode=True, simulation_scenario="receive_ok"), sim_time_scale=200
        )
        window.confirm = lambda _info, _parent: ConfirmOutcome(accepted=True)
        qtbot.addWidget(window)
        window.show()
        assert "SIMULATIONSMODUS" in window.header.mode_banner.text()

        source = tmp_path / "PROGRAMM_1234.NC"
        shutil.copy(fixtures_dir / "valid.nc", source)
        before = sha256_file(source)
        window.load_file(source)
        assert window.program is not None and not window.program.analysis.has_errors

        window.start_send()
        qtbot.waitUntil(lambda: not window.busy, timeout=15000)
        assert window.last_result is not None and window.last_result["state"] == "completed"

        window.start_receive()
        qtbot.waitUntil(lambda: not window.busy, timeout=15000)
        assert window.last_stored is not None
        stored = window.last_stored.nc_path
        assert stored.is_file()
        assert sha256_file(stored) == window.last_result["sha256_normalized"]
        assert sha256_file(source) == before

        assert window.close()
    finally:
        sys.excepthook = previous_hook
    assert unhandled == []
