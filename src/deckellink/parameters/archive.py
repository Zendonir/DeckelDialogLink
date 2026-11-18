"""Versioned storage of machine parameter sets (never overwritten)."""

from __future__ import annotations

import json
import logging
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Any

from deckellink.domain.models import Profile
from deckellink.domain.results import ReceivedProgram, ReceiveResult
from deckellink.storage.checksums import sha256_bytes
from deckellink.storage.files import FileExistsConflict, unique_path, write_new_file
from deckellink.version import __version__

log = logging.getLogger(__name__)

PARAM_SUFFIX = ".par"


@dataclass(frozen=True)
class StoredParameterSet:
    path: Path
    meta_path: Path
    raw_path: Path | None
    metadata: dict[str, Any]

    @property
    def complete(self) -> bool:
        return bool(self.metadata.get("complete"))

    @property
    def sha256(self) -> str:
        return str(self.metadata.get("sha256_normalized", ""))

    @property
    def read_at(self) -> datetime | None:
        value = self.metadata.get("received_at")
        return datetime.fromisoformat(value) if isinstance(value, str) else None


class ParameterArchive:
    """Stores parameter sets under ``<root>/parameters`` (raw data under ``parameters/raw``)."""

    def __init__(self, root: Path) -> None:
        self.directory = root / "parameters"
        self.raw_directory = self.directory / "raw"

    def store(
        self,
        result: ReceiveResult,
        program: ReceivedProgram,
        profile: Profile,
    ) -> StoredParameterSet | None:
        if not result.raw:
            return None
        when = result.first_byte_at or result.started_at
        stem = f"PARAM_{when.strftime('%Y%m%d_%H%M%S')}"
        self.directory.mkdir(parents=True, exist_ok=True)
        path = self._write_unique(self.directory, stem, PARAM_SUFFIX, program.normalized)
        raw_path = self._write_unique(self.raw_directory, path.stem, ".raw", program.raw)
        metadata: dict[str, Any] = {
            "type": "machine_parameters",
            "application": f"DeckelLink {__version__}",
            "received_at": when.isoformat(),
            "profile_id": profile.id,
            "port": result.port,
            "simulated": result.simulated,
            "raw_bytes": len(program.raw),
            "normalized_bytes": len(program.normalized),
            "sha256_raw": sha256_bytes(program.raw),
            "sha256_normalized": sha256_bytes(program.normalized),
            "end_reason": result.end_reason.value,
            "state": result.state.value,
            "complete": program.complete and program.decode_errors == 0,
            "decode_errors": program.decode_errors,
            "findings": [f.to_dict() for f in program.analysis.findings],
            "note": "Vollständigkeit nur anhand des Übertragungsendes beurteilt (Parameterformat nicht verifiziert).",
        }
        meta_path = path.with_suffix(".json")
        write_new_file(meta_path, (json.dumps(metadata, indent=2, ensure_ascii=False) + "\n").encode("utf-8"))
        log.info(
            "Machine parameters stored: %s (%d bytes, sha256=%s, complete=%s)",
            path.name,
            len(program.normalized),
            metadata["sha256_normalized"][:16],
            metadata["complete"],
        )
        return StoredParameterSet(path, meta_path, raw_path, metadata)

    @staticmethod
    def _write_unique(directory: Path, stem: str, suffix: str, data: bytes) -> Path:
        directory.mkdir(parents=True, exist_ok=True)
        for _ in range(5):
            path = unique_path(directory, stem, suffix)
            try:
                write_new_file(path, data)
                return path
            except FileExistsConflict:
                continue
        raise FileExistsConflict(f"Konnte keine eindeutige Datei für {stem}{suffix} anlegen")

    def list_sets(self) -> list[StoredParameterSet]:
        if not self.directory.is_dir():
            return []
        sets = []
        for path in self.directory.glob(f"*{PARAM_SUFFIX}"):
            meta_path = path.with_suffix(".json")
            metadata: dict[str, Any] = {}
            if meta_path.is_file():
                try:
                    loaded = json.loads(meta_path.read_text(encoding="utf-8"))
                    metadata = loaded if isinstance(loaded, dict) else {}
                except (OSError, json.JSONDecodeError):
                    metadata = {}
            raw = self.raw_directory / f"{path.stem}.raw"
            sets.append(StoredParameterSet(path, meta_path, raw if raw.is_file() else None, metadata))
        return sorted(sets, key=lambda s: (s.metadata.get("received_at") or "", s.path.name), reverse=True)
