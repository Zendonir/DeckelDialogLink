"""Create GitHub release notes for a version from CHANGELOG.md.

Usage: python tools/release_notes.py 0.4.0 > notes.md
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

HEADER = """\
> **Vorabversion.** Die Kommunikationsparameter und Protokolle der DIALOG 11 sind **nicht** an einer realen \
FP2A verifiziert; alle Tests liefen gegen den eingebauten Simulator. DeckelLink überträgt ausschließlich Daten \
und startet niemals Programme oder Bewegungen an der Maschine.

## Downloads
- `DeckelLink-{version}-Setup-x64.exe` – Installer für Windows 10/11 (64 Bit), ohne Administratorrechte
- `DeckelLink-{version}-windows-x64.zip` – portable Version (entpacken, `DeckelLink.exe` starten)
- `SHA256SUMS.txt` – Prüfsummen

Vor dem ersten Einsatz an der Maschine: `docs/COMMISSIONING.md` und `docs/VERIFICATION_CHECKLIST.md`.

"""


def changelog_section(changelog: str, version: str) -> str:
    """Return the body of the ``## [version]`` section (without the heading)."""
    pattern = re.compile(rf"^## \[{re.escape(version)}\][^\n]*\n(.*?)(?=^## \[|\Z)", re.MULTILINE | re.DOTALL)
    match = pattern.search(changelog)
    if match is None:
        raise ValueError(f"Version {version} nicht in CHANGELOG.md gefunden")
    return match.group(1).strip()


def build_notes(version: str, changelog: str) -> str:
    return HEADER.format(version=version) + "## Änderungen\n\n" + changelog_section(changelog, version) + "\n"


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("usage: release_notes.py <version>", file=sys.stderr)
        return 2
    changelog = (ROOT / "CHANGELOG.md").read_text(encoding="utf-8")
    sys.stdout.write(build_notes(argv[1], changelog))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
