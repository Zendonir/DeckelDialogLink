"""Diagnostic package export (ZIP).

Contains logs, anonymised configuration, the active profile, system info and
the last transfer result. NC files and raw data are only included when the
user explicitly selects them.
"""

from __future__ import annotations

import json
import logging
import platform
import re
import sys
import zipfile
from collections.abc import Iterable
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from deckellink.clock import Clock, SystemClock
from deckellink.config import AppConfig
from deckellink.domain.models import Profile
from deckellink.storage.files import unique_path
from deckellink.storage.paths import AppPaths
from deckellink.version import __version__

log = logging.getLogger(__name__)

_WIN_PATH_RE = re.compile(r"[A-Za-z]:\\[^\s\"'<>|]*")
_POSIX_PATH_RE = re.compile(r"(?<![\w.])/(?:home|Users|root|tmp|var|mnt|media)/[^\s\"'<>|]*")


def mask_paths(text: str) -> str:
    """Replace absolute paths by ``<PFAD>/<dateiname>``."""

    def repl(match: re.Match[str]) -> str:
        value = match.group(0).rstrip(".,;:)")
        tail = match.group(0)[len(value) :]
        name = re.split(r"[\\/]", value.rstrip("\\/"))[-1]
        return f"<PFAD>/{name}{tail}"

    text = _WIN_PATH_RE.sub(repl, text)
    return _POSIX_PATH_RE.sub(repl, text)


@dataclass
class ExportOptions:
    mask_paths: bool = True
    include_nc_files: list[Path] = field(default_factory=list)
    include_raw_files: list[Path] = field(default_factory=list)
    last_result: dict[str, Any] | None = None


def system_info() -> dict[str, Any]:
    return {
        "application": "DeckelLink",
        "version": __version__,
        "python": sys.version,
        "platform": platform.platform(),
        "machine": platform.machine(),
        "frozen": bool(getattr(sys, "frozen", False)),
    }


def anonymized_config(config: AppConfig) -> dict[str, Any]:
    data = config.to_dict()
    data["last_open_dir"] = "<entfernt>" if data.get("last_open_dir") else ""
    return data


def export_diagnostics(
    paths: AppPaths,
    config: AppConfig,
    profile: Profile,
    options: ExportOptions,
    *,
    clock: Clock | None = None,
    target_dir: Path | None = None,
) -> Path:
    clock = clock or SystemClock()
    target_dir = target_dir or paths.exports
    target_dir.mkdir(parents=True, exist_ok=True)
    stamp = clock.now().strftime("%Y%m%d_%H%M%S")
    target = unique_path(target_dir, f"DeckelLink_Diagnose_{stamp}", ".zip")

    def clean(text: str) -> str:
        return mask_paths(text) if options.mask_paths else text

    with zipfile.ZipFile(target, "x", compression=zipfile.ZIP_DEFLATED) as archive:
        archive.writestr("system.json", json.dumps(system_info(), indent=2))
        archive.writestr("config.json", clean(json.dumps(anonymized_config(config), indent=2)))
        archive.writestr("profile.json", json.dumps(profile.to_dict(), indent=2, ensure_ascii=False))
        if options.last_result is not None:
            archive.writestr("last_result.json", clean(json.dumps(options.last_result, indent=2, ensure_ascii=False)))
        for log_file in _log_files(paths.logs):
            try:
                text = log_file.read_text(encoding="utf-8", errors="replace")
            except OSError as exc:
                log.warning("Could not read log %s: %s", log_file.name, exc)
                continue
            archive.writestr(f"logs/{log_file.name}", clean(text))
        for nc in options.include_nc_files:
            if nc.is_file():
                archive.write(nc, f"nc/{nc.name}")
        for raw in options.include_raw_files:
            if raw.is_file():
                archive.write(raw, f"raw/{raw.name}")
        archive.writestr(
            "README.txt",
            "DeckelLink Diagnosepaket\n"
            f"Pfade maskiert: {'ja' if options.mask_paths else 'nein'}\n"
            f"NC-Dateien enthalten: {len(options.include_nc_files)}\n"
            f"Rohdaten enthalten: {len(options.include_raw_files)}\n",
        )
    log.info(
        "Diagnostic package exported: %s (nc=%d, raw=%d, masked=%s)",
        target.name,
        len(options.include_nc_files),
        len(options.include_raw_files),
        options.mask_paths,
    )
    return target


def _log_files(log_dir: Path) -> Iterable[Path]:
    if not log_dir.is_dir():
        return []
    return sorted(p for p in log_dir.iterdir() if p.is_file() and ".log" in p.name)
