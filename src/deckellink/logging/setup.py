"""Logging configuration with rotating log files.

NC file contents are never written to the standard log. Raw traffic logging is
a separate, explicitly enabled feature (see ``diagnostics.terminal``).
"""

from __future__ import annotations

import logging
import logging.handlers
import platform
import sys
from pathlib import Path

from deckellink.version import APP_NAME, __version__

LOG_FILE = "deckellink.log"
LOG_FORMAT = "%(asctime)s %(levelname)-7s [%(threadName)s] %(name)s: %(message)s"
_HANDLER_TAG = "_deckellink_handler"


def setup_logging(log_dir: Path, level: str = "INFO", *, console: bool = True) -> Path:
    """Configure the root logger. Safe to call repeatedly (replaces own handlers)."""
    log_dir.mkdir(parents=True, exist_ok=True)
    log_path = log_dir / LOG_FILE
    root = logging.getLogger()
    for handler in list(root.handlers):
        if getattr(handler, _HANDLER_TAG, False):
            root.removeHandler(handler)
            handler.close()

    file_handler = logging.handlers.RotatingFileHandler(
        log_path, maxBytes=2 * 1024 * 1024, backupCount=5, encoding="utf-8"
    )
    file_handler.setFormatter(logging.Formatter(LOG_FORMAT))
    file_handler.setLevel(logging.DEBUG)
    setattr(file_handler, _HANDLER_TAG, True)
    root.addHandler(file_handler)

    # A windowed (frozen) Windows build has no console: sys.stderr may be None.
    if console and sys.stderr is not None:
        stream_handler = logging.StreamHandler(sys.stderr)
        stream_handler.setFormatter(logging.Formatter(LOG_FORMAT))
        stream_handler.setLevel(logging.WARNING)
        setattr(stream_handler, _HANDLER_TAG, True)
        root.addHandler(stream_handler)

    set_level(level)
    return log_path


def set_level(level: str) -> None:
    logging.getLogger().setLevel(getattr(logging, level.upper(), logging.INFO))


def log_startup() -> None:
    log = logging.getLogger("deckellink")
    log.info("=" * 60)
    log.info("%s %s started", APP_NAME, __version__)
    log.info("OS: %s %s (%s)", platform.system(), platform.release(), platform.machine())
    log.info("Python: %s", sys.version.split()[0])
    log.info("Frozen: %s", bool(getattr(sys, "frozen", False)))


def log_shutdown() -> None:
    logging.getLogger("deckellink").info("%s %s stopped", APP_NAME, __version__)
