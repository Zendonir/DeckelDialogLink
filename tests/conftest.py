"""Shared fixtures. No test uses a real COM port."""

from __future__ import annotations

import hashlib
import os
from collections.abc import Iterator
from dataclasses import replace
from pathlib import Path

import pytest

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from deckellink.clock import FakeClock
from deckellink.domain.models import Profile
from deckellink.profiles.loader import builtin_profiles_dir
from deckellink.profiles.repository import ProfileRepository
from deckellink.storage.paths import AppPaths

FIXTURES = Path(__file__).parent / "fixtures"


@pytest.fixture
def fixtures_dir() -> Path:
    return FIXTURES


@pytest.fixture(scope="session", autouse=True)
def fixtures_unchanged() -> Iterator[None]:
    """Guarantee that no test (or application code) modifies an original fixture file."""
    before = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in FIXTURES.glob("*") if p.is_file()}
    yield
    after = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in FIXTURES.glob("*") if p.is_file()}
    assert before == after, "fixture files were modified"


@pytest.fixture
def clock() -> FakeClock:
    return FakeClock()


@pytest.fixture
def paths(tmp_path: Path) -> AppPaths:
    return AppPaths.from_root(tmp_path / "DeckelLink").ensure()


@pytest.fixture
def repo(paths: AppPaths) -> ProfileRepository:
    return ProfileRepository(builtin_profiles_dir(), paths.profiles)


@pytest.fixture
def profile_a(repo: ProfileRepository) -> Profile:
    profile = repo.get("dialog11_7n1_rtscts")
    assert profile is not None
    return profile


@pytest.fixture
def profile_b(repo: ProfileRepository) -> Profile:
    profile = repo.get("dialog11_7e1_none")
    assert profile is not None
    return profile


@pytest.fixture
def fast_profile(profile_a: Profile) -> Profile:
    """Profile A without start/end delays (shorter fake-clock runs)."""
    return replace(
        profile_a,
        transfer=replace(profile_a.transfer, start_delay_ms=0, end_delay_ms=0),
    )
