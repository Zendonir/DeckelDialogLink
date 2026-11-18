"""Application configuration (per user, JSON)."""

from __future__ import annotations

import json
import logging
import shutil
from dataclasses import asdict, dataclass, fields
from pathlib import Path
from typing import Any

from deckellink.storage.files import atomic_write_bytes

log = logging.getLogger(__name__)

CONFIG_FILE = "settings.json"
LOG_LEVELS = ("DEBUG", "INFO", "WARNING", "ERROR")


@dataclass
class AppConfig:
    port: str = ""
    connection_kind: str = "serial"  # serial | tcp | rfc2217 (RS-232/Ethernet adapter)
    network_host: str = ""
    network_port: int = 4001
    # Safe default: the application starts in simulation mode. Real transfers
    # require a conscious switch in the settings.
    simulation_mode: bool = True
    simulation_scenario: str = "send_ok"
    active_profile_id: str = "dialog11_7n1_rtscts"
    show_safety_notice: bool = True
    log_level: str = "INFO"
    raw_traffic_logging: bool = False
    terminal_history_lines: int = 2000
    mask_paths_in_export: bool = True
    backup_retention_days: int | None = None
    last_open_dir: str = ""

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> tuple[AppConfig, list[str]]:
        """Build a config, ignoring invalid values (reported as warnings)."""
        config = cls()
        warnings: list[str] = []
        for f in fields(cls):
            if f.name not in data:
                continue
            value = data[f.name]
            default = getattr(config, f.name)
            if f.name == "backup_retention_days":
                if value is None or (isinstance(value, int) and not isinstance(value, bool) and value > 0):
                    config.backup_retention_days = value
                else:
                    warnings.append(f"Ungültiger Wert für {f.name}: {value!r}")
                continue
            if isinstance(default, bool):
                ok = isinstance(value, bool)
            elif isinstance(default, int):
                ok = isinstance(value, int) and not isinstance(value, bool)
            else:
                ok = isinstance(value, str)
            if not ok:
                warnings.append(f"Ungültiger Wert für {f.name}: {value!r}")
                continue
            setattr(config, f.name, value)
        if config.connection_kind not in ("serial", "tcp", "rfc2217"):
            warnings.append(f"Unbekannte Verbindungsart {config.connection_kind!r}")
            config.connection_kind = "serial"
        if not 1 <= config.network_port <= 65535:
            warnings.append(f"Ungültiger TCP-Port {config.network_port}")
            config.network_port = 4001
        if config.log_level not in LOG_LEVELS:
            warnings.append(f"Unbekanntes Log-Level {config.log_level!r}")
            config.log_level = "INFO"
        config.terminal_history_lines = min(max(config.terminal_history_lines, 100), 100_000)
        return config, warnings


def load_config(config_dir: Path) -> tuple[AppConfig, list[str]]:
    path = config_dir / CONFIG_FILE
    if not path.is_file():
        return AppConfig(), []
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        log.error("Settings file unreadable, using defaults: %s", exc)
        return AppConfig(), [f"Einstellungen konnten nicht gelesen werden ({exc}); Standardwerte aktiv."]
    if not isinstance(data, dict):
        return AppConfig(), ["Einstellungsdatei hat ein ungültiges Format; Standardwerte aktiv."]
    config, warnings = AppConfig.from_dict(data)
    for warning in warnings:
        log.warning("Settings: %s", warning)
    return config, warnings


def save_config(config: AppConfig, config_dir: Path) -> None:
    """Save atomically; the previous version is kept as ``settings.json.bak``."""
    path = config_dir / CONFIG_FILE
    if path.is_file():
        shutil.copy2(path, path.with_suffix(".json.bak"))
    payload = (json.dumps(config.to_dict(), indent=2, ensure_ascii=False) + "\n").encode("utf-8")
    atomic_write_bytes(path, payload, overwrite=True)
