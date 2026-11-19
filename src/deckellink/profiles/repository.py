"""Profile repository: builtin (read-only) and user profiles."""

from __future__ import annotations

import json
import logging
import re
from dataclasses import replace
from pathlib import Path

from deckellink.domain.errors import DeckelLinkError, FieldError, ProfileValidationError
from deckellink.domain.models import FALLBACK_PROFILE, Profile
from deckellink.profiles.loader import ProfileLoadResult, load_profile_file
from deckellink.profiles.schema import parse_profile
from deckellink.storage.backup import BackupManager
from deckellink.storage.files import atomic_write_bytes

log = logging.getLogger(__name__)

PREFERRED_DEFAULT_ID = "dialog11_7n1_rtscts"


class ProfileRepositoryError(DeckelLinkError):
    """Invalid operation on the profile repository."""


class ProfileRepository:
    """Manages profiles. Builtin profiles are never modified or deleted."""

    def __init__(
        self,
        builtin_dir: Path,
        user_dir: Path,
        backups: BackupManager | None = None,
    ) -> None:
        self.builtin_dir = builtin_dir
        self.user_dir = user_dir
        self._backups = backups
        self._profiles: dict[str, Profile] = {}
        self._warnings: dict[str, list[str]] = {}
        self._paths: dict[str, Path] = {}
        self.problems: list[ProfileLoadResult] = []
        self.reload()

    # -- loading ---------------------------------------------------------
    def reload(self) -> list[ProfileLoadResult]:
        self._profiles.clear()
        self._warnings.clear()
        self._paths.clear()
        self.problems = []
        for path in sorted(self.builtin_dir.glob("*.json")) if self.builtin_dir.is_dir() else []:
            self._register(load_profile_file(path, builtin=True))
        if self.user_dir.is_dir():
            for path in sorted(self.user_dir.glob("*.json")):
                self._register(load_profile_file(path, builtin=False))
        return self.problems

    def _register(self, result: ProfileLoadResult) -> None:
        if result.profile is None:
            self.problems.append(result)
            return
        profile = result.profile
        if profile.id in self._profiles:
            result.error = (
                f"Profil-ID '{profile.id}' ist bereits vergeben ({self._paths[profile.id].name}); Datei wird ignoriert."
            )
            result.profile = None
            self.problems.append(result)
            log.warning("Duplicate profile id %s in %s", profile.id, result.path.name)
            return
        self._profiles[profile.id] = profile
        self._warnings[profile.id] = result.warnings
        self._paths[profile.id] = result.path

    # -- queries -----------------------------------------------------------
    @property
    def profiles(self) -> list[Profile]:
        builtin = [p for p in self._profiles.values() if p.builtin]
        user = sorted((p for p in self._profiles.values() if not p.builtin), key=lambda p: p.display_name)
        return builtin + user

    def get(self, profile_id: str) -> Profile | None:
        return self._profiles.get(profile_id)

    def warnings_for(self, profile_id: str) -> list[str]:
        return list(self._warnings.get(profile_id, []))

    def path_for(self, profile_id: str) -> Path | None:
        return self._paths.get(profile_id)

    def is_builtin(self, profile_id: str) -> bool:
        profile = self._profiles.get(profile_id)
        return profile is not None and profile.builtin

    def default_profile(self) -> Profile:
        if PREFERRED_DEFAULT_ID in self._profiles:
            return self._profiles[PREFERRED_DEFAULT_ID]
        builtin = [p for p in self._profiles.values() if p.builtin]
        if builtin:
            return builtin[0]
        log.error("No builtin profile could be loaded - using internal fallback profile")
        return FALLBACK_PROFILE

    def resolve(self, profile_id: str | None) -> tuple[Profile, bool]:
        """Return the requested profile or a safe default. Second value: fell back."""
        if profile_id and profile_id in self._profiles:
            return self._profiles[profile_id], False
        if profile_id:
            log.warning("Profile %s not available, falling back to default", profile_id)
        return self.default_profile(), bool(profile_id)

    # -- modification --------------------------------------------------------
    def unique_id(self, base: str) -> str:
        base = re.sub(r"[^a-z0-9_\-]", "_", base.lower())[:56] or "profil"
        candidate = base
        n = 1
        while candidate in self._profiles or (self.user_dir / f"{candidate}.json").exists():
            candidate = f"{base}_{n:02d}"
            n += 1
        return candidate

    def save(self, profile: Profile) -> tuple[Profile, list[str]]:
        """Persist ``profile`` as a user profile.

        If ``profile.id`` belongs to a builtin profile, a user copy with a new id
        is created instead - builtin profiles are never overwritten.
        Returns the stored profile and its warnings.
        """
        if self.is_builtin(profile.id) or profile.id == FALLBACK_PROFILE.id:
            new_id = self.unique_id(f"{profile.id}_benutzer")
            name = profile.display_name
            if "(angepasst)" not in name:
                name = f"{name} (angepasst)"
            profile = replace(profile, id=new_id, display_name=name[:120])
        profile = replace(profile, builtin=False)
        # Validate the serialized form so stored files are always loadable.
        data = profile.to_dict()
        validated, warnings = parse_profile(data, f"{profile.id}.json")
        self.user_dir.mkdir(parents=True, exist_ok=True)
        target = self.user_dir / f"{profile.id}.json"
        if target.exists() and self._backups is not None:
            self._backups.backup(target)
        payload = (json.dumps(data, indent=2, ensure_ascii=False) + "\n").encode("utf-8")
        atomic_write_bytes(target, payload, overwrite=True)
        self._profiles[validated.id] = validated
        self._warnings[validated.id] = warnings
        self._paths[validated.id] = target
        log.info("Profile saved: %s", validated.id)
        return validated, warnings

    def duplicate(self, profile_id: str, display_name: str | None = None) -> tuple[Profile, list[str]]:
        source = self.get(profile_id)
        if source is None:
            raise ProfileRepositoryError(f"Profil '{profile_id}' nicht gefunden.")
        new_id = self.unique_id(f"{source.id}_kopie")
        copy = replace(
            source,
            id=new_id,
            display_name=(display_name or f"{source.display_name} (Kopie)")[:120],
            builtin=False,
        )
        return self.save(copy)

    def delete(self, profile_id: str) -> None:
        if self.is_builtin(profile_id):
            raise ProfileRepositoryError("Mitgelieferte Standardprofile können nicht gelöscht werden.")
        path = self._paths.get(profile_id)
        if path is None:
            raise ProfileRepositoryError(f"Profil '{profile_id}' nicht gefunden.")
        if self._backups is not None:
            self._backups.backup(path)
        path.unlink()
        self._profiles.pop(profile_id, None)
        self._warnings.pop(profile_id, None)
        self._paths.pop(profile_id, None)
        log.info("Profile deleted (backup kept): %s", profile_id)

    # -- exchange ----------------------------------------------------------------
    def export_profile(self, profile_id: str, target: Path, *, overwrite: bool = False) -> Path | None:
        """Write a profile as JSON. Existing targets are only replaced with ``overwrite`` (backup first).

        Returns the backup path if one was created.
        """
        profile = self.get(profile_id)
        if profile is None:
            raise ProfileRepositoryError(f"Profil '{profile_id}' nicht gefunden.")
        backup: Path | None = None
        if target.exists():
            if not overwrite:
                raise ProfileRepositoryError(f"Datei existiert bereits: {target.name}")
            if self._backups is not None:
                backup = self._backups.backup(target)
        payload = (json.dumps(profile.to_dict(), indent=2, ensure_ascii=False) + "\n").encode("utf-8")
        atomic_write_bytes(target, payload, overwrite=True)
        log.info("Profile exported: %s -> %s", profile_id, target.name)
        return backup

    def import_profile(self, source: Path) -> tuple[Profile, list[str]]:
        """Validate a profile file and store it as a user profile (never as builtin).

        An id that already exists gets a new unique id; the verification status is kept
        as stored in the file but reported in the warnings.
        """
        result = load_profile_file(source)
        if result.profile is None:
            raise ProfileValidationError(
                result.field_errors or [FieldError("(Datei)", result.error or "")], source.name
            )
        profile = result.profile
        warnings = list(result.warnings)
        if profile.id in self._profiles or (self.user_dir / f"{profile.id}.json").exists():
            new_id = self.unique_id(f"{profile.id}_import")
            warnings.append(f"Profil-ID '{profile.id}' existiert bereits - importiert als '{new_id}'.")
            profile = replace(profile, id=new_id, display_name=f"{profile.display_name} (Import)"[:120])
        stored, save_warnings = self.save(replace(profile, builtin=False))
        if stored.verification_status.value == "verified":
            warnings.append("Das importierte Profil ist als 'verifiziert' gekennzeichnet - Herkunft prüfen.")
        log.info("Profile imported from %s as %s", source.name, stored.id)
        return stored, warnings + [w for w in save_warnings if w not in warnings]
