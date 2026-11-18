# Changelog

Format nach [Keep a Changelog](https://keepachangelog.com/de/1.1.0/), Versionierung nach [SemVer](https://semver.org/lang/de/).

## [0.3.0] – 2026-11-18

**Nur gegen den Simulator getestet.**

### Hinzugefügt
- Seite „Maschinenparameter": Auslesen als versionierte Sicherung (SHA-256, Rohdaten, Metadaten), Vergleich (Diff), Schreiben einer Parameterdatei in die an der CNC freigegebene Parametereingabe – über Klartext oder LSV2.
- Schreibschutz: Profilfreigabe `parameters.allow_write` (Standard aus), Freischaltung nur pro Sitzung, aktuelle Sicherung (≤ 60 min, gleiches Profil/Anschluss), Prüfung und Diff, getippte Bestätigung, automatische Kontrolle beim erneuten Auslesen.
- Profilabschnitt `parameters` (optional) und Reiter im Profil-Editor; Dokumentation `docs/PARAMETERS.md`.

### Nicht enthalten
- Fernbefehle zum Lesen/Ändern einzelner Parameter (Format/Befehle der DIALOG 11 nicht dokumentiert).

## [0.2.0] – 2026-11-18

**Nur gegen den Simulator getestet.**

### Hinzugefügt
- LSV2-Protokoll (DIN-66019-Basisverfahren): Verbindungsaufbau mit ENQ/DLE 0, Telegramme `DLE STX … DLE ETX` + BCC mit DLE-Transparenz, alternierende Quittungen, NAK-Wiederholung, ENQ-Nachfrage bei verlorener Quittung, WACK, EOT/RVI.
- NC-Nachladebetrieb (Drip-Feed) über LSV2, durch die Quittungen der CNC getaktet; eigener Zustand, Warnhinweis mit Pflichtbestätigung, Windows-Standby-Sperre.
- Senden und Empfangen über LSV2.
- Profilabschnitt `protocol` (optional, rückwärtskompatibel) und Profil `dialog11_lsv2_7e1` (nicht verifiziert); Reiter „Protokoll (LSV2)" im Profil-Editor.
- Simulierte LSV2-CNC mit Satzpuffer und 8 LSV2-Szenarien.
- Steuerzeichen im Terminal lesbar (`<ENQ>`, `<DLE>`, `<STX>` …).

### Geändert
- Drip-Feed ist nicht mehr Nichtziel; siehe `docs/DIALOG11.md` und `docs/SAFETY.md`.

## [0.1.0] – 2026-11-18

Erste Version. **Nur gegen den Simulator getestet – keine Verbindung zu einer realen DIALOG 11 geprüft.**

### Hinzugefügt
- Senden von NC-Programmen mit Zustandsautomat (Idle → Validating → Ready → OpeningPort → WaitingForHandshake → Sending → Finishing → Completed/Cancelled/Failed), Fortschritt, Abbruch, Timeouts.
- Empfang mit Endsequenz- und Inaktivitätserkennung, Größenlimit, ASCII/HEX-Vorschau, Archivierung mit Metadaten und SHA-256.
- Getrennte NC-Analyse (Info/Warnung/Fehler) und Transformation; Originaldateien bleiben unverändert.
- Zwei unverifizierte DIALOG-11-Profile (7N1 RTS/CTS, 7E1 ohne Handshake), Profil-Editor, Validierung, Kopie statt Überschreiben.
- Simulator mit 15 Szenarien, Loopback-Transport und Loopback-Test-Assistent.
- Terminal (ASCII/HEX/kombiniert), Diagnoseseite mit Modemleitungen, Diagnosepaket-Export.
- Rotierende Logs, Backups, platformdirs-Benutzerverzeichnisse.
- PySide6-GUI (Deutsch) mit deutlich gekennzeichnetem Simulations-/Realbetrieb.
- PyInstaller-Onedir-Build, GitHub-Actions-Workflows für CI und Windows-Build.
