"""Program management ("Programmverwaltung"): NC programs, macros and machine data files.

Every entry is stored as ``<id>.nc`` with metadata ``<id>.json`` in the library
folder. Nothing is overwritten silently: a new content version moves the
previous version into ``versions/<id>/`` first, and deleting moves the files to
``deleted/``.
"""

from __future__ import annotations

import json
import logging
import re
import shutil
from dataclasses import dataclass, field
from datetime import datetime
from enum import StrEnum
from pathlib import Path
from typing import Any

from deckellink.domain.errors import DeckelLinkError
from deckellink.storage.checksums import sha256_bytes
from deckellink.storage.files import atomic_write_bytes, sanitize_stem, unique_path, write_new_file

log = logging.getLogger(__name__)

SUFFIX = ".nc"
_TOOL_RE = re.compile(r"(?<![A-Z])(TT|T)\s*(\d+)(?![\d.])")


class EntryKind(StrEnum):
    PROGRAM = "program"
    MACRO = "macro"
    TOOLS = "tools"
    OTHER = "other"

    @property
    def label(self) -> str:
        return {
            "program": "Hauptprogramm",
            "macro": "Makroprogramm",
            "tools": "Werkzeugdatei",
            "other": "Sonstige Datei",
        }[self.value]


class LibraryError(DeckelLinkError):
    """Library operation failed."""


@dataclass
class LibraryEntry:
    id: str
    path: Path
    name: str
    kind: EntryKind = EntryKind.PROGRAM
    number: str = ""
    description: str = ""
    source: str = ""
    created_at: str = ""
    updated_at: str = ""
    sha256: str = ""
    size: int = 0
    tools: list[str] = field(default_factory=list)
    last_sent_at: str = ""
    last_sent_mode: str = ""
    version_count: int = 0

    @property
    def meta_path(self) -> Path:
        return self.path.with_suffix(".json")

    def read(self) -> bytes:
        return self.path.read_bytes()

    def to_meta(self) -> dict[str, Any]:
        return {
            "name": self.name,
            "kind": self.kind.value,
            "number": self.number,
            "description": self.description,
            "source": self.source,
            "created_at": self.created_at,
            "updated_at": self.updated_at,
            "sha256": self.sha256,
            "size": self.size,
            "tools": self.tools,
            "last_sent_at": self.last_sent_at,
            "last_sent_mode": self.last_sent_mode,
        }


def tools_used(data: bytes) -> list[str]:
    """Tool calls ``T..``/``TT..`` in program text (comments in brackets ignored)."""
    text = re.sub(r"\([^()]*\)", " ", data.decode("ascii", errors="replace").upper())
    found: list[str] = []
    for prefix, number in _TOOL_RE.findall(text):
        label = f"{prefix}{int(number)}"
        if int(number) > 0 and label not in found:
            found.append(label)
    return found


def guess_kind(data: bytes, name: str) -> EntryKind:
    text = data[:4000].decode("ascii", errors="replace").upper()
    if re.search(r"(?m)^\s*%%\d", text) or name.upper().startswith("%%"):
        return EntryKind.MACRO
    if re.search(r"(?m)^%M[DP]G\d\d", text):
        return EntryKind.OTHER
    return EntryKind.PROGRAM


class ProgramLibrary:
    def __init__(self, root: Path) -> None:
        self.root = root
        self.versions_dir = root / "versions"
        self.deleted_dir = root / "deleted"

    def ensure(self) -> None:
        self.root.mkdir(parents=True, exist_ok=True)

    # ------------------------------------------------------------------ reading
    def entries(self) -> list[LibraryEntry]:
        if not self.root.is_dir():
            return []
        entries = [self._load(path) for path in sorted(self.root.glob(f"*{SUFFIX}"))]
        return sorted(entries, key=lambda e: (e.kind.value, e.name.lower()))

    def get(self, entry_id: str) -> LibraryEntry | None:
        path = self.root / f"{entry_id}{SUFFIX}"
        return self._load(path) if path.is_file() else None

    def _load(self, path: Path) -> LibraryEntry:
        entry = LibraryEntry(id=path.stem, path=path, name=path.stem)
        meta_path = path.with_suffix(".json")
        meta: dict[str, Any] = {}
        if meta_path.is_file():
            try:
                loaded = json.loads(meta_path.read_text(encoding="utf-8"))
                meta = loaded if isinstance(loaded, dict) else {}
            except (OSError, json.JSONDecodeError) as exc:
                log.warning("Library metadata unreadable for %s: %s", path.name, exc)
        entry.name = str(meta.get("name") or path.stem)
        try:
            entry.kind = EntryKind(str(meta.get("kind", "program")))
        except ValueError:
            entry.kind = EntryKind.OTHER
        for key in (
            "number",
            "description",
            "source",
            "created_at",
            "updated_at",
            "sha256",
            "last_sent_at",
            "last_sent_mode",
        ):
            setattr(entry, key, str(meta.get(key, "")))
        tools = meta.get("tools", [])
        entry.tools = [str(t) for t in tools] if isinstance(tools, list) else []
        try:
            entry.size = path.stat().st_size
        except OSError:
            entry.size = 0
        version_dir = self.versions_dir / path.stem
        entry.version_count = len(list(version_dir.glob(f"*{SUFFIX}"))) if version_dir.is_dir() else 0
        return entry

    # ------------------------------------------------------------------ writing
    def import_bytes(
        self, name: str, data: bytes, *, kind: EntryKind | None = None, source: str = "", now: datetime
    ) -> LibraryEntry:
        if not data:
            raise LibraryError("Leere Datei kann nicht übernommen werden.")
        self.ensure()
        stem = sanitize_stem(Path(name).stem, "PROGRAMM")
        path = unique_path(self.root, stem, SUFFIX)
        write_new_file(path, data)
        entry = LibraryEntry(
            id=path.stem,
            path=path,
            name=Path(name).stem or path.stem,
            kind=kind or guess_kind(data, name),
            source=source,
            created_at=now.isoformat(timespec="seconds"),
            updated_at=now.isoformat(timespec="seconds"),
            sha256=sha256_bytes(data),
            size=len(data),
            tools=tools_used(data),
        )
        self._write_meta(entry)
        log.info("Library: imported %s as %s (%d bytes, kind=%s)", name, path.name, len(data), entry.kind.value)
        return entry

    def import_file(self, source: Path, *, kind: EntryKind | None = None, now: datetime) -> LibraryEntry:
        try:
            data = source.read_bytes()
        except OSError as exc:
            raise LibraryError(f"Datei konnte nicht gelesen werden: {exc}") from exc
        return self.import_bytes(source.name, data, kind=kind, source=str(source.name), now=now)

    def save_version(self, entry: LibraryEntry, data: bytes, *, now: datetime) -> LibraryEntry:
        """Store new content; the previous content is kept as a version first."""
        if not data:
            raise LibraryError("Leerer Inhalt wird nicht gespeichert.")
        current = entry.read()
        if current == data:
            return entry
        self._keep_version(entry, current, now)
        atomic_write_bytes(entry.path, data, overwrite=True)
        entry.sha256 = sha256_bytes(data)
        entry.size = len(data)
        entry.tools = tools_used(data)
        entry.updated_at = now.isoformat(timespec="seconds")
        self._write_meta(entry)
        log.info("Library: new version of %s (%d bytes)", entry.path.name, len(data))
        return self.get(entry.id) or entry

    def _keep_version(self, entry: LibraryEntry, data: bytes, now: datetime) -> Path:
        directory = self.versions_dir / entry.id
        directory.mkdir(parents=True, exist_ok=True)
        path = unique_path(directory, now.strftime("%Y%m%d_%H%M%S"), SUFFIX)
        write_new_file(path, data)
        return path

    def versions(self, entry: LibraryEntry) -> list[Path]:
        directory = self.versions_dir / entry.id
        return sorted(directory.glob(f"*{SUFFIX}"), reverse=True) if directory.is_dir() else []

    def restore_version(self, entry: LibraryEntry, version: Path, *, now: datetime) -> LibraryEntry:
        if version.parent != self.versions_dir / entry.id:
            raise LibraryError("Version gehört nicht zu diesem Eintrag.")
        return self.save_version(entry, version.read_bytes(), now=now)

    def update_meta(self, entry: LibraryEntry, **values: str) -> LibraryEntry:
        for key, value in values.items():
            if key == "kind":
                entry.kind = EntryKind(value)
            elif key in ("name", "number", "description"):
                setattr(entry, key, value.strip())
            else:
                raise LibraryError(f"Unbekanntes Feld {key}")
        self._write_meta(entry)
        return entry

    def mark_sent(self, entry: LibraryEntry, *, now: datetime, mode: str) -> None:
        entry.last_sent_at = now.isoformat(timespec="seconds")
        entry.last_sent_mode = mode
        self._write_meta(entry)

    def delete(self, entry: LibraryEntry, *, now: datetime) -> Path:
        """Move the entry (file, metadata, versions) into ``deleted/`` - never a hard delete."""
        target = self.deleted_dir / f"{entry.id}_{now.strftime('%Y%m%d_%H%M%S')}"
        n = 1
        while target.exists():
            target = target.with_name(f"{target.name}_{n}")
            n += 1
        target.mkdir(parents=True)
        shutil.move(str(entry.path), target / entry.path.name)
        if entry.meta_path.is_file():
            shutil.move(str(entry.meta_path), target / entry.meta_path.name)
        versions = self.versions_dir / entry.id
        if versions.is_dir():
            shutil.move(str(versions), target / "versions")
        log.info("Library: %s moved to %s", entry.path.name, target.name)
        return target

    def _write_meta(self, entry: LibraryEntry) -> None:
        text = json.dumps(entry.to_meta(), ensure_ascii=False, indent=2)
        atomic_write_bytes(entry.meta_path, text.encode("utf-8"), overwrite=True)
