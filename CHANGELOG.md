# Changelog

Format nach [Keep a Changelog](https://keepachangelog.com/de/1.1.0/), Versionierung nach [SemVer](https://semver.org/lang/de/).

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
