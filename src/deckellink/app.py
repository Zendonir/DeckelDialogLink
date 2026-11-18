"""Application entry point."""

from __future__ import annotations

import argparse
import logging
import sys
from collections.abc import Sequence
from pathlib import Path

from deckellink.config import load_config
from deckellink.logging.setup import log_shutdown, log_startup, setup_logging
from deckellink.storage.paths import AppPaths
from deckellink.version import APP_NAME, __version__

log = logging.getLogger("deckellink")


def parse_args(argv: Sequence[str] | None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        prog="deckellink",
        description="DeckelLink - NC-Datenübertragung für DECKEL FP2A / DIALOG 11 (nur Datenübertragung).",
    )
    parser.add_argument("--version", action="version", version=f"{APP_NAME} {__version__}")
    parser.add_argument("--data-dir", type=Path, help="Datenverzeichnis überschreiben (portabler Betrieb)")
    parser.add_argument(
        "--simulate",
        action="store_true",
        help="Im Simulationsmodus starten (überschreibt die gespeicherte Betriebsart für diese Sitzung)",
    )
    parser.add_argument(
        "--smoke-test",
        action="store_true",
        help="Headless-Selbsttest im Simulator ausführen und beenden (Exit-Code 0 = OK)",
    )
    parser.add_argument("--smoke-report", type=Path, help="JSON-Bericht des Selbsttests schreiben")
    return parser.parse_args(list(argv) if argv is not None else None)


def run_smoke(paths: AppPaths, report: Path | None) -> int:
    from deckellink.simulation.runner import run_smoke_test

    result = run_smoke_test(paths, report)
    if sys.stdout is not None:
        print(result.to_json())
    log.info("Smoke test finished: ok=%s", result.ok)
    return 0 if result.ok else 1


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv)
    paths = AppPaths.default(args.data_dir).ensure()
    config, config_warnings = load_config(paths.config)
    setup_logging(paths.logs, config.log_level)
    log_startup()
    for warning in config_warnings:
        log.warning("Settings: %s", warning)
    if args.smoke_test:
        code = run_smoke(paths, args.smoke_report)
        log_shutdown()
        return code
    if args.simulate:
        config.simulation_mode = True

    from PySide6.QtGui import QIcon
    from PySide6.QtWidgets import QApplication

    from deckellink.gui.main_window import MainWindow
    from deckellink.gui.style import apply_theme

    app = QApplication.instance() or QApplication(sys.argv)
    assert isinstance(app, QApplication)
    app.setApplicationName(APP_NAME)
    app.setApplicationVersion(__version__)
    app.setOrganizationName(APP_NAME)
    apply_theme(app)
    icon_path = Path(__file__).resolve().parent / "resources" / "icons" / "deckellink.png"
    if icon_path.is_file():
        app.setWindowIcon(QIcon(str(icon_path)))

    def excepthook(exc_type: type[BaseException], exc: BaseException, tb: object) -> None:
        log.critical("Unhandled exception", exc_info=(exc_type, exc, tb))  # type: ignore[arg-type]

    sys.excepthook = excepthook
    window = MainWindow(paths, config)
    window.show()
    code = int(app.exec())
    log_shutdown()
    return code


if __name__ == "__main__":
    sys.exit(main())
