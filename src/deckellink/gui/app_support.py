"""Application-level helpers: unexpected-error dialog, single instance, documentation."""

from __future__ import annotations

import logging
import sys
import threading
import traceback
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path
from types import TracebackType

from PySide6.QtCore import QLockFile, QThread, QTimer
from PySide6.QtWidgets import QApplication, QMessageBox, QWidget

from deckellink.storage.paths import AppPaths

log = logging.getLogger("deckellink")

LOCK_FILE = "deckellink.lock"


# --------------------------------------------------------------------------- errors
def format_unexpected_error(exc_type: type[BaseException], exc: BaseException, tb: TracebackType | None) -> str:
    return "".join(traceback.format_exception(exc_type, exc, tb))


def show_unexpected_error(details: str, parent: QWidget | None = None) -> None:
    box = QMessageBox(parent)
    box.setIcon(QMessageBox.Icon.Critical)
    box.setWindowTitle("Unerwarteter Fehler")
    box.setText(
        "In DeckelLink ist ein unerwarteter Fehler aufgetreten.\n\n"
        "Laufende Übertragungen wurden nicht automatisch fortgesetzt. Prüfen Sie den Zustand an der "
        "CNC. Bitte exportieren Sie unter 'Diagnose → Diagnosepaket exportieren' ein Diagnosepaket "
        "und melden Sie den Fehler."
    )
    box.setDetailedText(details)
    box.exec()


def install_exception_hooks(
    show: Callable[[str], None] | None = None,
) -> Callable[[type[BaseException], BaseException, TracebackType | None], None]:
    """Log every unhandled exception; in the GUI thread also show a dialog."""
    shown = {"active": False}
    display = show or show_unexpected_error

    def hook(exc_type: type[BaseException], exc: BaseException, tb: TracebackType | None) -> None:
        log.critical("Unhandled exception", exc_info=(exc_type, exc, tb))
        app = QApplication.instance()
        if app is None or QThread.currentThread() is not app.thread() or shown["active"]:
            return
        details = format_unexpected_error(exc_type, exc, tb)

        def present() -> None:
            shown["active"] = True
            try:
                display(details)
            finally:
                shown["active"] = False

        QTimer.singleShot(0, present)

    def thread_hook(args: threading.ExceptHookArgs) -> None:
        log.critical(
            "Unhandled exception in thread %s",
            args.thread.name if args.thread else "?",
            exc_info=(args.exc_type, args.exc_value or args.exc_type(), args.exc_traceback),
        )

    sys.excepthook = hook
    threading.excepthook = thread_hook
    return hook


# --------------------------------------------------------------------------- single instance
def acquire_instance_lock(paths: AppPaths) -> QLockFile | None:
    """Return a held lock, or ``None`` if another DeckelLink instance uses this data folder."""
    paths.root.mkdir(parents=True, exist_ok=True)
    lock = QLockFile(str(paths.root / LOCK_FILE))
    lock.setStaleLockTime(0)  # a lock of a crashed process is detected via its PID
    if lock.tryLock(200):
        return lock
    if lock.removeStaleLockFile() and lock.tryLock(200):
        return lock
    return None


# --------------------------------------------------------------------------- documentation
@dataclass(frozen=True)
class HelpDocument:
    file_name: str
    title: str


HELP_DOCUMENTS: tuple[HelpDocument, ...] = (
    HelpDocument("README.md", "Übersicht und Bedienung"),
    HelpDocument("SAFETY.md", "Sicherheitskonzept"),
    HelpDocument("COMMISSIONING.md", "Inbetriebnahme"),
    HelpDocument("VERIFICATION_CHECKLIST.md", "Verifikations-Checkliste"),
    HelpDocument("DIALOG11.md", "DIALOG 11 - bekannte/angenommene Eigenschaften"),
    HelpDocument("CABLE.md", "Kabel, Adapter, Netzwerkadapter"),
    HelpDocument("PARAMETERS.md", "Maschinenparameter"),
    HelpDocument("TROUBLESHOOTING.md", "Fehlersuche"),
    HelpDocument("INSTALLATION.md", "Installation und Datenordner"),
    HelpDocument("ARCHITECTURE.md", "Technische Architektur"),
)


def docs_dir() -> Path:
    """Folder with the bundled user documentation (frozen build or source checkout)."""
    base = getattr(sys, "_MEIPASS", None)
    if base:
        bundled = Path(base) / "deckellink" / "resources" / "docs"
        if bundled.is_dir():
            return bundled
    package = Path(__file__).resolve().parent.parent
    bundled = package / "resources" / "docs"
    if bundled.is_dir():
        return bundled
    return package.parent.parent / "docs"


def find_document(name: str) -> Path | None:
    folder = docs_dir()
    for candidate in (folder / name, folder.parent / name):
        if candidate.is_file():
            return candidate
    return None
