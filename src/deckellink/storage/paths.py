"""User-specific directories (never the program directory)."""

from __future__ import annotations

import os
from dataclasses import dataclass
from pathlib import Path

from platformdirs import user_data_dir, user_log_dir

from deckellink.version import APP_NAME

ENV_HOME = "DECKELLINK_HOME"


@dataclass(frozen=True)
class AppPaths:
    root: Path
    config: Path
    logs: Path
    received: Path
    backups: Path
    failed: Path
    raw: Path
    exports: Path
    profiles: Path
    simulation: Path

    @classmethod
    def from_root(cls, root: Path, logs: Path | None = None) -> AppPaths:
        return cls(
            root=root,
            config=root / "config",
            logs=logs or root / "logs",
            received=root / "received",
            backups=root / "backups",
            failed=root / "failed",
            raw=root / "raw",
            exports=root / "exports",
            profiles=root / "profiles",
            simulation=root / "simulation",
        )

    @classmethod
    def default(cls, override: Path | None = None) -> AppPaths:
        """Resolve paths: explicit override > ``DECKELLINK_HOME`` > platformdirs."""
        if override is not None:
            return cls.from_root(override)
        env = os.environ.get(ENV_HOME)
        if env:
            return cls.from_root(Path(env))
        root = Path(user_data_dir(APP_NAME, appauthor=False))
        logs = Path(user_log_dir(APP_NAME, appauthor=False))
        return cls.from_root(root, logs=logs)

    def ensure(self) -> AppPaths:
        for path in (
            self.root,
            self.config,
            self.logs,
            self.received,
            self.backups,
            self.failed,
            self.raw,
            self.exports,
            self.profiles,
            self.simulation,
        ):
            path.mkdir(parents=True, exist_ok=True)
        return self
