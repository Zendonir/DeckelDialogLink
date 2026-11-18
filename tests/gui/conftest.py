from __future__ import annotations

from collections.abc import Callable, Iterator

import pytest
from pytestqt.qtbot import QtBot

from deckellink.config import AppConfig
from deckellink.gui.dialogs.confirm_transfer import ConfirmInfo, ConfirmOutcome
from deckellink.gui.main_window import MainWindow
from deckellink.storage.paths import AppPaths


class TestMainWindow(MainWindow):
    """MainWindow that records confirmation requests instead of showing a dialog."""

    __test__ = False
    confirmations: list[ConfirmInfo]


WindowFactory = Callable[..., TestMainWindow]


@pytest.fixture
def make_window(qtbot: QtBot, paths: AppPaths) -> Iterator[WindowFactory]:
    created: list[TestMainWindow] = []

    def factory(config: AppConfig | None = None, scale: float = 200.0, accept: bool = True) -> TestMainWindow:
        window = TestMainWindow(paths, config or AppConfig(), sim_time_scale=scale)
        window.confirmations = []

        def confirm(info: ConfirmInfo, _parent: object) -> ConfirmOutcome:
            window.confirmations.append(info)
            return ConfirmOutcome(accepted=accept)

        window.confirm = confirm
        qtbot.addWidget(window)
        window.show()
        created.append(window)
        return window

    yield factory
    for window in created:
        window.shutdown()


def wait_idle(qtbot: QtBot, window: MainWindow, timeout: int = 15000) -> None:
    qtbot.waitUntil(lambda: not window.busy, timeout=timeout)
    qtbot.wait(20)
