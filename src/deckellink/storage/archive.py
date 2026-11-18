"""Versioned archive for received programs."""

from __future__ import annotations

import json
import logging
import re
import shutil
from dataclasses import asdict, dataclass
from datetime import datetime
from pathlib import Path
from typing import Any

from deckellink.clock import Clock, SystemClock
from deckellink.domain.models import Profile, ReceiveSettings
from deckellink.domain.results import ReceivedProgram, ReceiveResult
from deckellink.storage.backup import BackupManager
from deckellink.storage.checksums import sha256_bytes, sha256_file
from deckellink.storage.files import (
    FileExistsConflict,
    atomic_write_bytes,
    sanitize_stem,
    unique_path,
    write_new_file,
)
from deckellink.storage.paths import AppPaths
from deckellink.version import __version__

log = logging.getLogger(__name__)

NC_SUFFIX = ".nc"


@dataclass(frozen=True)
class StoredReceive:
    nc_path: Path
    meta_path: Path
    raw_path: Path | None
    complete: bool
    metadata: dict[str, Any]


@dataclass(frozen=True)
class ArchivedProgram:
    path: Path
    folder: str
    size: int
    modified: datetime
    metadata: dict[str, Any] | None

    @property
    def complete(self) -> bool | None:
        if self.metadata is None:
            return None
        return bool(self.metadata.get("complete"))


def derive_stem(data: bytes, settings: ReceiveSettings, when: datetime) -> str:
    """Program name from the data stream if a profile pattern finds one, else RECEIVED_<ts>.

    The DIALOG 11 program-number format is not verified, so no pattern is
    active by default.
    """
    if settings.program_name_pattern:
        try:
            match = re.search(settings.program_name_pattern, data.decode("ascii", errors="replace"))
        except re.error:
            match = None
        if match and match.group(1):
            name = sanitize_stem(match.group(1), fallback="")
            if name:
                return name
    return f"RECEIVED_{when.strftime('%Y%m%d_%H%M%S')}"


class ReceiveArchive:
    def __init__(self, paths: AppPaths, clock: Clock | None = None) -> None:
        self.paths = paths
        self.clock = clock or SystemClock()

    def store(
        self,
        result: ReceiveResult,
        program: ReceivedProgram,
        profile: Profile,
    ) -> StoredReceive | None:
        """Store the received program. Returns ``None`` if nothing was received.

        Complete programs go to ``received/``, incomplete ones to ``failed/``.
        Raw bytes are stored separately in ``raw/`` when the profile requests it.
        Existing files are never overwritten.
        """
        if not result.raw:
            return None
        when = result.first_byte_at or result.started_at
        stem = derive_stem(program.normalized, profile.receive, when)
        target_dir = self.paths.received if program.complete else self.paths.failed
        target_dir.mkdir(parents=True, exist_ok=True)

        nc_path = self._write_unique(target_dir, stem, NC_SUFFIX, program.normalized)
        final_stem = nc_path.stem
        raw_path: Path | None = None
        if profile.receive.save_raw:
            raw_path = self._write_unique(self.paths.raw, final_stem, ".raw", program.raw)

        metadata: dict[str, Any] = {
            "application": f"DeckelLink {__version__}",
            "received_at": when.isoformat(),
            "started_at": result.started_at.isoformat(),
            "last_byte_at": result.last_byte_at.isoformat() if result.last_byte_at else None,
            "duration_s": round(result.duration_s, 3),
            "profile_id": profile.id,
            "profile_verification_status": profile.verification_status.value,
            "port": result.port,
            "simulated": result.simulated,
            "serial": asdict(profile.serial) | {"parity": profile.serial.parity.value},
            "raw_bytes": len(program.raw),
            "normalized_bytes": len(program.normalized),
            "sha256_raw": sha256_bytes(program.raw),
            "sha256_normalized": sha256_bytes(program.normalized),
            "end_reason": result.end_reason.value,
            "state": result.state.value,
            "complete": program.complete,
            "decode_errors": program.decode_errors,
            "notes": list(program.notes),
            "findings": [f.to_dict() for f in program.analysis.findings],
            "nc_file": nc_path.name,
            "raw_file": str(raw_path.relative_to(self.paths.root)) if raw_path else None,
        }
        meta_bytes = (json.dumps(metadata, indent=2, ensure_ascii=False) + "\n").encode("utf-8")
        meta_path = nc_path.with_suffix(".json")
        write_new_file(meta_path, meta_bytes)
        log.info(
            "Received program stored: %s (%d raw bytes, sha256=%s, complete=%s)",
            nc_path.name,
            len(program.raw),
            metadata["sha256_normalized"][:16],
            program.complete,
        )
        return StoredReceive(nc_path, meta_path, raw_path, program.complete, metadata)

    def _write_unique(self, directory: Path, stem: str, suffix: str, data: bytes) -> Path:
        for _ in range(5):
            path = unique_path(directory, stem, suffix)
            if suffix == NC_SUFFIX and path.with_suffix(".json").exists():
                stem = path.stem + "_x"
                continue
            try:
                write_new_file(path, data)
                return path
            except FileExistsConflict:
                continue
        raise FileExistsConflict(f"Konnte keine eindeutige Datei für {stem}{suffix} anlegen")

    def list_programs(self) -> list[ArchivedProgram]:
        programs: list[ArchivedProgram] = []
        for folder_name, folder in (("received", self.paths.received), ("failed", self.paths.failed)):
            if not folder.is_dir():
                continue
            for path in folder.glob(f"*{NC_SUFFIX}"):
                meta_path = path.with_suffix(".json")
                metadata: dict[str, Any] | None = None
                if meta_path.is_file():
                    try:
                        loaded = json.loads(meta_path.read_text(encoding="utf-8"))
                        metadata = loaded if isinstance(loaded, dict) else None
                    except (OSError, json.JSONDecodeError):
                        metadata = None
                stat = path.stat()
                programs.append(
                    ArchivedProgram(
                        path=path,
                        folder=folder_name,
                        size=stat.st_size,
                        modified=datetime.fromtimestamp(stat.st_mtime).astimezone(),
                        metadata=metadata,
                    )
                )
        return sorted(programs, key=lambda p: p.modified, reverse=True)


def export_copy(source: Path, target: Path, backups: BackupManager, *, overwrite: bool) -> Path | None:
    """Copy ``source`` to ``target``.

    If ``target`` exists it is only replaced when ``overwrite`` is true, and a
    backup is created first. Returns the backup path (if one was created).
    """
    backup_path: Path | None = None
    if source.resolve() == target.resolve():
        raise FileExistsConflict("Quelle und Ziel sind identisch.")
    if target.exists():
        if not overwrite:
            raise FileExistsConflict(f"Datei existiert bereits: {target.name}")
        backup_path = backups.backup(target)
    data = source.read_bytes()
    atomic_write_bytes(target, data, overwrite=True)
    if sha256_file(target) != sha256_bytes(data):
        raise OSError(f"Prüfsumme nach dem Kopieren stimmt nicht: {target}")
    shutil.copystat(source, target)
    log.info("Exported %s -> %s", source.name, target.name)
    return backup_path
