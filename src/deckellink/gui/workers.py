"""Background execution with QThread + worker objects.

Rules enforced here:
* the job runs outside the GUI thread,
* the job never touches widgets - it only emits signals (queued to the GUI),
* cancellation is cooperative through a :class:`CancellationToken`,
* exceptions are converted into a :class:`UserFacingError` and never crash Qt.
"""

from __future__ import annotations

import logging
from collections.abc import Callable
from dataclasses import dataclass

from PySide6.QtCore import QCoreApplication, QObject, QThread, Signal, Slot

from deckellink.diagnostics.error_messages import describe_error
from deckellink.transfer.cancellation import CancellationToken

log = logging.getLogger(__name__)


@dataclass
class TaskCallbacks:
    """Thread-safe callbacks handed to a job; they only emit Qt signals."""

    state: Callable[[object], None]
    progress: Callable[[object], None]
    traffic: Callable[[object], None]
    modem: Callable[[object], None]


Job = Callable[[TaskCallbacks, CancellationToken], object]


class TaskWorker(QObject):
    state_changed = Signal(object)
    progress = Signal(object)
    traffic = Signal(object)
    modem = Signal(object)
    finished = Signal(object)
    crashed = Signal(object)

    def __init__(self, name: str, job: Job, token: CancellationToken) -> None:
        super().__init__()
        self.name = name
        self._job = job
        self._token = token

    @Slot()
    def run(self) -> None:
        callbacks = TaskCallbacks(
            state=self.state_changed.emit,
            progress=self.progress.emit,
            traffic=self.traffic.emit,
            modem=self.modem.emit,
        )
        result: object = None
        error: object = None
        try:
            result = self._job(callbacks, self._token)
        except Exception as exc:
            log.exception("Background task %s crashed", self.name)
            error = describe_error(exc, operation=self.name)
        app = QCoreApplication.instance()
        if app is not None:
            # Hand the worker back to the GUI thread so it is destroyed there.
            self.moveToThread(app.thread())
        if error is not None:
            self.crashed.emit(error)
        else:
            self.finished.emit(result)


class TaskHandle:
    """Owns the thread and worker of one background task."""

    def __init__(self, name: str, job: Job) -> None:
        self.name = name
        self.token = CancellationToken()
        self.thread = QThread()
        self.thread.setObjectName(f"DeckelLink-{name}")
        self.worker = TaskWorker(name, job, self.token)
        self.worker.moveToThread(self.thread)
        self.thread.started.connect(self.worker.run)
        self.worker.finished.connect(self.thread.quit)
        self.worker.crashed.connect(self.thread.quit)
        self._started = False

    def start(self) -> None:
        self._started = True
        self.thread.start()

    def cancel(self) -> None:
        self.token.cancel()

    @property
    def is_running(self) -> bool:
        return self._started and self.thread.isRunning()

    def wait(self, timeout_ms: int = 5000) -> bool:
        if not self._started:
            return True
        return bool(self.thread.wait(timeout_ms))
