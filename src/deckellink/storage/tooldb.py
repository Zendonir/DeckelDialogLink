"""Tool management ("Werkzeugverwaltung") of DeckelLink.

The tool table is kept on the PC (``tools.json``). It feeds the simulator and
the tool check of programs. The format in which the DIALOG 11 reads or outputs
tool data is NOT known; a tool file for the machine can therefore only be
produced from a user-defined template (marked unverified) - or a tool file that
was received from the machine is resent unchanged.
"""

from __future__ import annotations

import json
import logging
import math
from dataclasses import asdict, dataclass, fields
from pathlib import Path
from typing import Any

from deckellink.domain.errors import DeckelLinkError
from deckellink.storage.backup import BackupManager
from deckellink.storage.files import atomic_write_bytes

log = logging.getLogger(__name__)

TOOL_FILE = "tools.json"
PLACEHOLDERS = ("{T}", "{NAME}", "{R}", "{D}", "{L}", "{Z}", "{F}", "{S}", "{TYP}")


class ToolDataError(DeckelLinkError):
    """Invalid tool data or template."""


@dataclass(frozen=True)
class Tool:
    number: int
    name: str = ""
    kind: str = "Fräser"
    diameter: float = 10.0
    length: float = 0.0
    flutes: int = 0
    feed: float = 0.0
    speed: float = 0.0
    note: str = ""

    @property
    def radius(self) -> float:
        return self.diameter / 2.0

    def problems(self) -> list[str]:
        out: list[str] = []
        if self.number <= 0 or self.number > 9999:
            out.append(f"T{self.number}: Werkzeugnummer muss zwischen 1 und 9999 liegen")
        if not math.isfinite(self.diameter) or self.diameter < 0 or self.diameter > 1000:
            out.append(f"T{self.number}: Durchmesser {self.diameter} ist unplausibel")
        if not math.isfinite(self.length) or abs(self.length) > 2000:
            out.append(f"T{self.number}: Länge {self.length} ist unplausibel")
        if self.feed < 0 or self.speed < 0 or self.flutes < 0:
            out.append(f"T{self.number}: negative Werte sind nicht zulässig")
        return out


def _tool_from_dict(data: dict[str, Any]) -> Tool:
    kwargs: dict[str, Any] = {}
    for f in fields(Tool):
        if f.name not in data:
            continue
        value = data[f.name]
        if f.type in ("int",):
            kwargs[f.name] = int(value)
        elif f.type in ("float",):
            kwargs[f.name] = float(value)
        else:
            kwargs[f.name] = str(value)
    if "number" not in kwargs:
        raise ToolDataError("Werkzeug ohne Nummer")
    return Tool(**kwargs)


class ToolDatabase:
    def __init__(self, path: Path, backups: BackupManager | None = None) -> None:
        self.path = path
        self.backups = backups
        self.tools: list[Tool] = []
        self.problems: list[str] = []

    def load(self) -> list[Tool]:
        self.tools = []
        self.problems = []
        if not self.path.is_file():
            return self.tools
        try:
            data = json.loads(self.path.read_text(encoding="utf-8"))
            items = data.get("tools", []) if isinstance(data, dict) else []
        except (OSError, json.JSONDecodeError) as exc:
            self.problems.append(f"Werkzeugdatei nicht lesbar: {exc}")
            return self.tools
        for item in items:
            try:
                self.tools.append(_tool_from_dict(item))
            except (ToolDataError, TypeError, ValueError) as exc:
                self.problems.append(f"Werkzeug übersprungen: {exc}")
        self.tools.sort(key=lambda t: t.number)
        return self.tools

    def save(self, tools: list[Tool]) -> None:
        problems = [p for t in tools for p in t.problems()]
        numbers = [t.number for t in tools]
        duplicates = sorted({n for n in numbers if numbers.count(n) > 1})
        if duplicates:
            problems.append("Doppelte Werkzeugnummern: " + ", ".join(f"T{n}" for n in duplicates))
        if problems:
            raise ToolDataError("; ".join(problems))
        if self.path.is_file() and self.backups is not None:
            self.backups.backup(self.path)
        payload = {"schema_version": 1, "tools": [asdict(t) for t in sorted(tools, key=lambda t: t.number)]}
        self.path.parent.mkdir(parents=True, exist_ok=True)
        atomic_write_bytes(self.path, json.dumps(payload, ensure_ascii=False, indent=2).encode("utf-8"), overwrite=True)
        self.tools = sorted(tools, key=lambda t: t.number)
        log.info("Tool table saved (%d tools)", len(tools))

    def get(self, number: int) -> Tool | None:
        return next((t for t in self.tools if t.number == number), None)


@dataclass(frozen=True)
class ToolExportTemplate:
    """User-defined text layout for a tool file (unverified for the DIALOG 11)."""

    header: str = ""
    line: str = ""
    footer: str = ""

    @property
    def configured(self) -> bool:
        return bool(self.line.strip())


def check_program_tools(used: list[str], tools: list[Tool]) -> list[str]:
    """Tool calls of a program that are missing in the tool table."""
    known = {t.number for t in tools}
    missing = []
    for label in used:
        number = int(label.lstrip("T"))
        if number not in known:
            missing.append(label)
    return missing


def export_tool_file(tools: list[Tool], template: ToolExportTemplate) -> bytes:
    """Build a tool file text from ``template``. Raises if not configured or not ASCII."""
    if not template.configured:
        raise ToolDataError(
            "Keine Vorlage hinterlegt. Das Werkzeugdatei-Format der DIALOG 11 ist nicht bekannt - "
            "bitte zuerst eine Werkzeugdatei von der Steuerung auslesen und danach die Vorlage einrichten."
        )
    if not tools:
        raise ToolDataError("Werkzeugtabelle ist leer.")
    lines: list[str] = []
    if template.header.strip():
        lines.extend(template.header.splitlines())
    for tool in sorted(tools, key=lambda t: t.number):
        values = {
            "{T}": str(tool.number),
            "{NAME}": tool.name,
            "{R}": _fmt(tool.radius),
            "{D}": _fmt(tool.diameter),
            "{L}": _fmt(tool.length),
            "{Z}": str(tool.flutes),
            "{F}": _fmt(tool.feed),
            "{S}": _fmt(tool.speed),
            "{TYP}": tool.kind,
        }
        text = template.line
        for key, value in values.items():
            text = text.replace(key, value)
        lines.append(text)
    if template.footer.strip():
        lines.extend(template.footer.splitlines())
    result = "\r\n".join(lines) + "\r\n"
    try:
        return result.encode("ascii")
    except UnicodeEncodeError as exc:
        raise ToolDataError("Die Werkzeugdatei enthält Zeichen außerhalb von 7-Bit-ASCII (z. B. Umlaute).") from exc


def _fmt(value: float) -> str:
    text = f"{value:.3f}".rstrip("0").rstrip(".")
    return text if text not in ("", "-0") else "0"
