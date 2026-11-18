from __future__ import annotations

import importlib.util
from pathlib import Path
from types import ModuleType

import pytest

from deckellink.version import __version__

ROOT = Path(__file__).resolve().parents[2]


def load_tool() -> ModuleType:
    spec = importlib.util.spec_from_file_location("release_notes", ROOT / "tools" / "release_notes.py")
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_changelog_has_current_version_and_notes_build() -> None:
    tool = load_tool()
    notes = tool.build_notes(__version__, (ROOT / "CHANGELOG.md").read_text(encoding="utf-8"))
    assert "Vorabversion" in notes
    assert f"DeckelLink-{__version__}-Setup-x64.exe" in notes
    assert "## Änderungen" in notes
    assert f"## [{__version__}]" not in notes


def test_section_extraction() -> None:
    tool = load_tool()
    text = "# X\n\n## [2.0.0] – d\n\nneu\n\n## [1.0.0] – d\n\nalt\n"
    assert tool.changelog_section(text, "2.0.0") == "neu"
    assert tool.changelog_section(text, "1.0.0") == "alt"
    with pytest.raises(ValueError, match=r"3\.0\.0"):
        tool.changelog_section(text, "3.0.0")


def test_installer_script_consistent() -> None:
    script = (ROOT / "installer" / "deckellink.iss").read_text(encoding="utf-8")
    assert "PrivilegesRequired=lowest" in script
    assert "OutputBaseFilename={#AppName}-{#AppVersion}-Setup-x64" in script
    assert "LOCALAPPDATA%\\DeckelLink" in script  # user data is documented as preserved
    assert (ROOT / "installer" / "info_before_de.txt").read_bytes().startswith(b"\xef\xbb\xbf")
