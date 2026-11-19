"""Commissioning assistant: steps of docs/COMMISSIONING.md, progress per profile, report.

Marking a profile as "verified" is only possible when every required step is
done and a tester is named. DeckelLink never changes machine settings here; the
assistant documents what the operator did.
"""

from __future__ import annotations

import json
import logging
import shutil
from dataclasses import asdict, dataclass, field
from datetime import datetime
from enum import StrEnum
from pathlib import Path
from typing import Any

from deckellink.domain.models import Profile
from deckellink.storage.files import atomic_write_bytes, sanitize_stem, unique_path, write_new_file
from deckellink.version import __version__

log = logging.getLogger(__name__)


class StepStatus(StrEnum):
    OPEN = "open"
    DONE = "done"
    NOT_APPLICABLE = "na"

    @property
    def label(self) -> str:
        return {"open": "offen", "done": "erledigt", "na": "nicht zutreffend"}[self.value]


@dataclass(frozen=True)
class Step:
    id: str
    title: str
    description: str
    required: bool = True
    action: str = ""  # loopback | format | receive | send | parameters | help


STEPS: tuple[Step, ...] = (
    Step(
        "connector",
        "Maschinenanschluss identifizieren",
        "Steckerform, Geschlecht und Belegung aus den Maschinenunterlagen ermitteln. Nichts ausprobieren.",
        action="help",
    ),
    Step(
        "cnc_settings",
        "Einstellungen an der CNC dokumentieren",
        "Baudrate, Datenbits, Parität, Stoppbits und Handshake der DIALOG 11 ablesen und notieren.",
    ),
    Step(
        "profile",
        "Profil auswählen bzw. anlegen",
        "Profil mit den dokumentierten Werten wählen oder als Kopie anlegen (Einstellungen).",
    ),
    Step(
        "cable",
        "Kabel ohne Maschine prüfen",
        "Durchgang und Belegung mit Messgerät gegen die Dokumentation prüfen; COM-Port/Adapter notieren.",
    ),
    Step(
        "loopback",
        "Loopback-Test bestanden",
        "Kabel von der Maschine trennen, TX/RX brücken, Loopback-Test im Realbetrieb.",
        action="loopback",
    ),
    Step(
        "format",
        "Datenformat durch Mitlesen bestätigt (optional)",
        "CNC gibt ein Programm aus, DeckelLink liest nur mit und schätzt Parität/Datenbits.",
        required=False,
        action="format",
    ),
    Step(
        "receive",
        "Programm von der CNC empfangen",
        "Zuerst Empfangsrichtung: ein bekanntes Programm an der CNC ausgeben und empfangen.",
        action="receive",
    ),
    Step(
        "raw",
        "Rohbytes analysiert",
        "HEX-Ansicht/Terminal prüfen: Zeichen korrekt, Zeilenende, Zeichen vor dem Programm und nach M30.",
    ),
    Step("backup", "Empfangene Datei gesichert", "Datei unter NC-Programme prüfen und zusätzlich sichern."),
    Step(
        "send_small",
        "Kleines, ungefährliches Testprogramm gesendet",
        "Kurzes Programm ohne kritische Bewegungen unter neuer Programmnummer senden.",
        action="send",
    ),
    Step(
        "verify_cnc",
        "Programm an der CNC vollständig geprüft",
        "Programm an der Steuerung mit dem Original vergleichen (Zeichen, Zeilen, Programmende).",
    ),
    Step(
        "no_autostart",
        "Erster Lauf nach betrieblichen Vorgaben",
        "Ausführung nur durch den Bediener an der CNC, ohne Werkstück bzw. Probelauf/Einzelsatz.",
    ),
    Step(
        "lsv2",
        "Nachladebetrieb (LSV2) geprüft (nur falls genutzt)",
        "Nach Abschnitt 12 der Inbetriebnahme-Anleitung; sonst 'nicht zutreffend'.",
        required=False,
        action="help",
    ),
    Step(
        "parameters",
        "Parameter-Sicherung erstellt (optional)",
        "Maschinenparameter auslesen und als Sicherung ablegen.",
        required=False,
        action="parameters",
    ),
)
STEP_IDS = tuple(step.id for step in STEPS)


@dataclass
class StepRecord:
    status: StepStatus = StepStatus.OPEN
    note: str = ""
    updated_at: str = ""


@dataclass
class CommissioningRecord:
    profile_id: str
    tester: str = ""
    machine_info: str = ""
    started_at: str = ""
    steps: dict[str, StepRecord] = field(default_factory=dict)

    def step(self, step_id: str) -> StepRecord:
        if step_id not in self.steps:
            self.steps[step_id] = StepRecord()
        return self.steps[step_id]

    def set_status(self, step_id: str, status: StepStatus, when: datetime) -> None:
        record = self.step(step_id)
        record.status = status
        record.updated_at = when.isoformat(timespec="seconds")

    def missing_required(self) -> list[Step]:
        return [s for s in STEPS if s.required and self.step(s.id).status is not StepStatus.DONE]

    def can_mark_verified(self) -> bool:
        return not self.missing_required() and bool(self.tester.strip())

    def done_count(self) -> int:
        return sum(1 for s in STEPS if self.step(s.id).status is not StepStatus.OPEN)

    def to_dict(self) -> dict[str, Any]:
        return {
            "profile_id": self.profile_id,
            "tester": self.tester,
            "machine_info": self.machine_info,
            "started_at": self.started_at,
            "steps": {k: asdict(v) | {"status": v.status.value} for k, v in self.steps.items()},
        }

    @classmethod
    def from_dict(cls, data: dict[str, Any], profile_id: str) -> CommissioningRecord:
        record = cls(
            profile_id=profile_id,
            tester=str(data.get("tester", "")),
            machine_info=str(data.get("machine_info", "")),
            started_at=str(data.get("started_at", "")),
        )
        steps = data.get("steps", {})
        if isinstance(steps, dict):
            for key, value in steps.items():
                if key not in STEP_IDS or not isinstance(value, dict):
                    continue
                status_raw = value.get("status", "open")
                status = StepStatus(status_raw) if status_raw in [s.value for s in StepStatus] else StepStatus.OPEN
                record.steps[key] = StepRecord(status, str(value.get("note", "")), str(value.get("updated_at", "")))
        return record


class CommissioningStore:
    """Progress per profile in ``<data>/commissioning/<profile_id>.json`` (previous version kept)."""

    def __init__(self, root: Path) -> None:
        self.directory = root / "commissioning"

    def path_for(self, profile_id: str) -> Path:
        return self.directory / f"{sanitize_stem(profile_id, 'profil')}.json"

    def load(self, profile_id: str, now: datetime) -> CommissioningRecord:
        path = self.path_for(profile_id)
        if path.is_file():
            try:
                data = json.loads(path.read_text(encoding="utf-8"))
                if isinstance(data, dict):
                    return CommissioningRecord.from_dict(data, profile_id)
            except (OSError, json.JSONDecodeError) as exc:
                log.error("Commissioning record %s unreadable: %s", path.name, exc)
        return CommissioningRecord(profile_id=profile_id, started_at=now.isoformat(timespec="seconds"))

    def save(self, record: CommissioningRecord) -> Path:
        path = self.path_for(record.profile_id)
        path.parent.mkdir(parents=True, exist_ok=True)
        if path.is_file():
            shutil.copy2(path, path.with_suffix(".json.bak"))
        payload = (json.dumps(record.to_dict(), indent=2, ensure_ascii=False) + "\n").encode("utf-8")
        atomic_write_bytes(path, payload, overwrite=True)
        return path


def report_markdown(record: CommissioningRecord, profile: Profile, now: datetime) -> str:
    s = profile.serial
    lines = [
        f"# Inbetriebnahme-Protokoll DeckelLink {__version__}",
        "",
        f"- Erstellt: {now.isoformat(timespec='seconds')}",
        f"- Prüfer: {record.tester or '-'}",
        f"- Maschine/Steuerungsstand: {record.machine_info or '-'}",
        f"- Profil: {profile.display_name} (`{profile.id}`), Status: {profile.verification_status.label}",
        f"- Schnittstelle: {s.summary}, Protokoll: {profile.protocol.mode.label}",
        f"- Begonnen: {record.started_at or '-'}",
        "",
        "| # | Schritt | Pflicht | Status | Notiz | Zeitpunkt |",
        "|---|---|---|---|---|---|",
    ]
    for index, step in enumerate(STEPS, start=1):
        rec = record.step(step.id)
        note = rec.note.replace("|", "/").replace("\n", " ")
        lines.append(
            f"| {index} | {step.title} | {'ja' if step.required else 'nein'} | {rec.status.label} | "
            f"{note} | {rec.updated_at} |"
        )
    missing = record.missing_required()
    lines.append("")
    if missing:
        lines.append("Offene Pflichtschritte: " + ", ".join(step.title for step in missing))
    else:
        lines.append("Alle Pflichtschritte sind erledigt.")
    lines.append("")
    lines.append(
        "Hinweis: Dieses Protokoll dokumentiert die Inbetriebnahme der Datenübertragung. Es ersetzt keine "
        "Prüfung der NC-Programme oder der Maschine nach betrieblichen Vorgaben."
    )
    return "\n".join(lines) + "\n"


def export_report(record: CommissioningRecord, profile: Profile, directory: Path, now: datetime) -> Path:
    stem = f"Inbetriebnahme_{sanitize_stem(profile.id, 'profil')}_{now.strftime('%Y%m%d_%H%M%S')}"
    target = unique_path(directory, stem, ".md")
    write_new_file(target, report_markdown(record, profile, now).encode("utf-8"))
    return target
