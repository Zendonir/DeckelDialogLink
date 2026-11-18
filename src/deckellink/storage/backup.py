"""Backups before replacing files. Never deletes silently."""

from __future__ import annotations

import logging
import shutil
from datetime import datetime, timedelta
from pathlib import Path

from deckellink.clock import Clock, SystemClock
from deckellink.storage.files import unique_path

log = logging.getLogger(__name__)


class BackupManager:
    """Copies files into ``backups/YYYY-MM-DD/NAME_HH-MM-SS.ext`` before they change.

    Retention is disabled by default (``retention_days=None``). Even when
    enabled, deletion only happens when :meth:`apply_retention` is called
    explicitly and every deleted file is logged.
    """

    def __init__(
        self,
        backup_dir: Path,
        clock: Clock | None = None,
        retention_days: int | None = None,
    ) -> None:
        self.backup_dir = backup_dir
        self.clock = clock or SystemClock()
        self.retention_days = retention_days

    def backup(self, source: Path) -> Path:
        now = self.clock.now()
        day_dir = self.backup_dir / now.strftime("%Y-%m-%d")
        day_dir.mkdir(parents=True, exist_ok=True)
        stem = f"{source.stem}_{now.strftime('%H-%M-%S')}"
        target = unique_path(day_dir, stem, source.suffix)
        shutil.copy2(source, target)
        log.info("Backup created: %s -> %s", source.name, target.relative_to(self.backup_dir))
        return target

    def list_backups(self) -> list[Path]:
        if not self.backup_dir.is_dir():
            return []
        return sorted(p for p in self.backup_dir.rglob("*") if p.is_file())

    def expired_backups(self) -> list[Path]:
        if self.retention_days is None:
            return []
        cutoff = (self.clock.now() - timedelta(days=self.retention_days)).date()
        expired = []
        for day_dir in sorted(self.backup_dir.iterdir()) if self.backup_dir.is_dir() else []:
            try:
                day = datetime.strptime(day_dir.name, "%Y-%m-%d").date()
            except ValueError:
                continue
            if day_dir.is_dir() and day < cutoff:
                expired.extend(sorted(p for p in day_dir.rglob("*") if p.is_file()))
        return expired

    def apply_retention(self) -> list[Path]:
        """Delete backups older than ``retention_days``. No-op when retention is disabled."""
        deleted = []
        for path in self.expired_backups():
            path.unlink()
            deleted.append(path)
            log.warning("Backup deleted by retention policy: %s", path.name)
        if deleted:
            for day_dir in self.backup_dir.iterdir():
                if day_dir.is_dir() and not any(day_dir.iterdir()):
                    day_dir.rmdir()
        return deleted
