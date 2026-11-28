# CLAUDE.md – Projektregeln für DeckelLink

Diese Regeln gelten dauerhaft für alle Änderungen in diesem Repository.

## Sprache
- Benutzeroberfläche: **Deutsch**.
- Code, Bezeichner, technische Kommentare, Log-Meldungen: **Englisch**.
- Benutzerdokumentation (`README.md`, `docs/`): **Deutsch**.

## Technik
- Python **3.12+**, PySide6, pyserial, platformdirs. Keine Web-/Cloud-Komponenten. Netzwerk nur als ausgehende Verbindung zu einem RS-232/Ethernet-Adapter im lokalen Maschinennetz (TCP-Rohdaten oder RFC 2217); DeckelLink öffnet keine eingehenden Ports.
- Vollständige Typannotationen für alle öffentlichen Funktionen; `mypy` läuft im Strict-Modus.
- Die GUI spricht nie direkt mit pyserial – nur über `deckellink.transport.base.Transport`.
- **Keine blockierenden Operationen im GUI-Thread.** Port-I/O läuft in `gui/workers.py` (QThread + Worker); Worker greifen nie auf Widgets zu, sondern senden Signale.
- Zeit immer über `deckellink.clock.Clock` (testbar mit `FakeClock`).

## Sicherheit (nicht verhandelbar)
- **Kein automatischer Maschinenstart**, keine Ausführungs- oder Steuerbefehle, keine Achs- oder Modusfunktionen.
- Kein automatisches Senden beim Programmstart oder nach Dateiauswahl; Bestätigung unmittelbar vor jedem Transfer.
- **Originaldateien niemals verändern.** Transformationen erzeugen separate Daten.
- **Keine stillen Dateiüberschreibungen** (`storage/files.py`: `write_new_file`, `unique_path`; vor Ersetzen Backup).
- Reale Übertragung und Simulation müssen **optisch eindeutig** unterscheidbar sein.
- **Keine NC-Dateiinhalte in Standardlogs.** Rohdatenprotokoll nur nach expliziter Aktivierung.

## DIALOG 11
- **Keine erfundenen DIALOG-11-Protokolleigenschaften.** Unbekanntes bleibt konfigurierbar, mit konservativem Standard, gekennzeichnet im Code und in `docs/VERIFICATION_CHECKLIST.md`.
- M30 nur als konfigurierbares Programmende; M82 nie ohne Profileinstellung.
- Nachladebetrieb nur über LSV2 und nur CNC-gesteuert (nächstes Telegramm erst nach Quittung); DeckelLink startet nie die Bearbeitung. LSV2-Details (Rahmung, BCC, Telegramminhalt) bleiben konfigurierbar und unverifiziert, bis an der FP2A geprüft.
- Maschinenparameter nur als Datei: Auslesen nach Ausgabe an der CNC, Schreiben nur in eine an der CNC freigegebene Parametereingabe. Keine Fernbefehle zum Lesen/Ändern von Parametern. Die Schreibsperren (Profil `parameters.allow_write` standardmäßig aus, Freischaltung nur pro Sitzung, aktuelle Sicherung, Diff, getippte Bestätigung, Kontrolle durch erneutes Auslesen) dürfen nicht abgeschwächt werden.
- Mitgelieferte Profile (`src/deckellink/resources/profiles/`) bleiben `unverified`, bis eine Prüfung an der realen FP2A dokumentiert ist.

## Rust-Neubau (`rust/`)
- Vom Benutzer freigegeben (2026-11-26): schrittweiser Neubau in Rust für maximale Stabilität; die Python-Version bleibt bis zur Gleichwertigkeit bestehen. Alle Sicherheitsregeln oben gelten unverändert auch für Rust.
- Kernbibliothek ohne `unsafe`, ohne `unwrap`/`expect`/`panic`/ungeprüfte Indizes (Workspace-Lints); Fehler als Werte mit deutscher Meldung.
- Mitgelieferte Profile nur in `src/deckellink/resources/profiles/` pflegen (Rust bindet sie ein).
- Verhalten muss mit Python übereinstimmen: `python tools/rust_parity.py` (nach `cargo build --release`) muss 0 Abweichungen melden.
- Vor Abschluss: `cd rust && cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test --all`.

## Qualität – vor jedem Abschluss ausführen
```bash
export QT_QPA_PLATFORM=offscreen   # headless
ruff check src tests
ruff format --check src tests
mypy
pytest --cov=deckellink
```
- Tests nach relevanten Änderungen ausführen; keine echten COM-Ports in automatisierten Tests.
- Keine Platzhalter (TODO, FIXME, `pass` als Implementierung).
