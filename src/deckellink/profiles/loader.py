"""Loading profile files without ever crashing the application."""

from __future__ import annotations

import json
import logging
import sys
from dataclasses import dataclass, field
from pathlib import Path

from deckellink.domain.errors import FieldError, ProfileValidationError
from deckellink.domain.models import Profile
from deckellink.profiles.schema import parse_profile

log = logging.getLogger(__name__)


@dataclass
class ProfileLoadResult:
    path: Path
    profile: Profile | None = None
    warnings: list[str] = field(default_factory=list)
    error: str | None = None
    field_errors: list[FieldError] = field(default_factory=list)

    @property
    def ok(self) -> bool:
        return self.profile is not None


def builtin_profiles_dir() -> Path:
    """Directory of the profiles shipped with the application (also when frozen)."""
    base = getattr(sys, "_MEIPASS", None)
    if base:
        frozen = Path(base) / "deckellink" / "resources" / "profiles"
        if frozen.is_dir():
            return frozen
    return Path(__file__).resolve().parent.parent / "resources" / "profiles"


def load_profile_file(path: Path, *, builtin: bool = False) -> ProfileLoadResult:
    result = ProfileLoadResult(path=path)
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as exc:
        result.error = f"Datei konnte nicht gelesen werden: {exc}"
        log.error("Profile %s unreadable: %s", path.name, exc)
        return result
    try:
        data = json.loads(text)
    except json.JSONDecodeError as exc:
        result.error = f"Ungültiges JSON in Zeile {exc.lineno}, Spalte {exc.colno}: {exc.msg}"
        result.field_errors = [FieldError("(JSON)", result.error)]
        log.error("Profile %s: invalid JSON (%s)", path.name, exc)
        return result
    try:
        profile, warnings = parse_profile(data, path.name, builtin=builtin)
    except ProfileValidationError as exc:
        result.error = "Profil ungültig: " + "; ".join(str(e) for e in exc.errors)
        result.field_errors = list(exc.errors)
        log.error("Profile %s invalid: %s", path.name, result.error)
        return result
    result.profile = profile
    result.warnings = warnings
    return result
