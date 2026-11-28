"""Create GitHub release notes for a version from CHANGELOG.md.

Usage: python tools/release_notes.py 0.4.0 > notes.md
       python tools/release_notes.py --rust 0.8.0-alpha.10 > notes.md   (section "Rust <version>")
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


RUST_HEADER = """\
> **Vorabversion der Rust-Neuentwicklung.** Gleicher Funktionsumfang und gleiche Sicherheitsregeln wie die \
Python-Version, deren Verhalten per Gleichwertigkeitstest abgesichert ist. Die Kommunikationsparameter und \
Protokolle der DIALOG 11 sind **nicht** an einer realen FP2A verifiziert; alle Tests liefen gegen den eingebauten \
Simulator. DeckelLink überträgt ausschließlich Daten und startet niemals Programme oder Bewegungen an der Maschine.

## Downloads
- `DeckelLink-Rust-{version}-Setup-x64.exe` – Installer für Windows 10/11 (64 Bit), ohne Administratorrechte; \
installiert neben einer vorhandenen Python-Version, gemeinsame Daten
- `DeckelLink-Rust-{version}-windows-x64.zip` – portable Version (entpacken, `deckellink.exe` starten)
- `SHA256SUMS.txt` – Prüfsummen

Selbsttest ohne Fenster: `deckellink.exe --smoke-test --smoke-report %TEMP%\\dlr.json` (Exit-Code 0 = OK).
Vor dem ersten Einsatz an der Maschine: `docs/COMMISSIONING.md` und `docs/VERIFICATION_CHECKLIST.md`.

"""


def changelog_section(changelog: str, version: str) -> str:
    """Return the body of the ``## [version]`` section (without the heading)."""
    pattern = re.compile(rf"^## \[{re.escape(version)}\][^\n]*\n(.*?)(?=^## \[|\Z)", re.MULTILINE | re.DOTALL)
    match = pattern.search(changelog)
    if match is None:
        raise ValueError(f"Version {version} nicht in CHANGELOG.md gefunden")
    return match.group(1).strip()


def build_notes(version: str, changelog: str, *, rust: bool = False) -> str:
    if rust:
        body = changelog_section(changelog, f"Rust {version}")
        return RUST_HEADER.format(version=version) + "## Änderungen\n\n" + body + "\n"
    return HEADER.format(version=version) + "## Änderungen\n\n" + changelog_section(changelog, version) + "\n"


def main(argv: list[str]) -> int:
    args = argv[1:]
    rust = "--rust" in args
    args = [a for a in args if a != "--rust"]
    if len(args) != 1:
        print("usage: release_notes.py [--rust] <version>", file=sys.stderr)
        return 2
    changelog = (ROOT / "CHANGELOG.md").read_text(encoding="utf-8")
    sys.stdout.write(build_notes(args[0], changelog, rust=rust))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
