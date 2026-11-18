"""Safe file writing: no silent overwrites, atomic replacement."""

from __future__ import annotations

import os
import re
import tempfile
from pathlib import Path

from deckellink.domain.errors import DeckelLinkError

_UNSAFE_NAME_RE = re.compile(r"[^A-Za-z0-9_\-]")


class FileExistsConflict(DeckelLinkError):
    """Target exists and overwriting was not explicitly requested."""


def sanitize_stem(stem: str, fallback: str = "PROGRAMM") -> str:
    cleaned = _UNSAFE_NAME_RE.sub("_", stem.strip())[:64].strip("_")
    return cleaned or fallback


def unique_path(directory: Path, stem: str, suffix: str) -> Path:
    """Deterministic non-conflicting name: NAME.nc, NAME_001.nc, NAME_002.nc ..."""
    candidate = directory / f"{stem}{suffix}"
    n = 1
    while candidate.exists():
        candidate = directory / f"{stem}_{n:03d}{suffix}"
        n += 1
        if n > 9999:
            raise FileExistsConflict(f"Kein freier Dateiname für {stem}{suffix} in {directory}")
    return candidate


def write_new_file(path: Path, data: bytes) -> None:
    """Create ``path`` exclusively. Raises ``FileExistsConflict`` if it exists."""
    path.parent.mkdir(parents=True, exist_ok=True)
    try:
        with path.open("xb") as handle:
            handle.write(data)
            handle.flush()
            os.fsync(handle.fileno())
    except FileExistsError as exc:
        raise FileExistsConflict(f"Datei existiert bereits: {path.name}") from exc


def atomic_write_bytes(path: Path, data: bytes, *, overwrite: bool = False) -> None:
    """Write via temporary file + rename. Refuses to replace unless ``overwrite``."""
    if path.exists() and not overwrite:
        raise FileExistsConflict(f"Datei existiert bereits: {path.name}")
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp_name = tempfile.mkstemp(prefix=f".{path.name}.", suffix=".tmp", dir=path.parent)
    tmp = Path(tmp_name)
    try:
        with os.fdopen(fd, "wb") as handle:
            handle.write(data)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(tmp, path)
    except BaseException:
        tmp.unlink(missing_ok=True)
        raise
