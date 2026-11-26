# Changelog

Format nach [Keep a Changelog](https://keepachangelog.com/de/1.1.0/), Versionierung nach [SemVer](https://semver.org/lang/de/).

## [Rust 0.8.0-alpha.1] – 2026-11-26

Beginn des Neubaus in Rust (Verzeichnis `rust/`, siehe `rust/README.md`). Die Python-Version bleibt die Anwendung mit Oberfläche.

### Hinzugefügt
- Kernbibliothek `deckellink-core` (Etappe 1). Enthält: Profile im gleichen JSON-Format mit derselben Prüfung und denselben Sicherheitshinweisen, NC-Prüfung und -Aufbereitung (EOT, M30/M2), Ports (seriell, TCP-Netzwerkadapter), simulierte CNC, Loopback, Senden und Empfangen.
- Kommandozeile `deckellink-cli` (`check`, `prepare`, `ports`, `send`, `receive`). Reales Senden nur nach getippter Bestätigung `SENDEN`.
- Stabilität: `unsafe` sowie `unwrap`/`panic`/ungeprüfte Indizes per Lint verboten, Überlaufprüfung auch im Release-Build, Eigenschaftstests mit Zufallsdaten, CI unter Linux und Windows (Windows-`.exe` als Artefakt).
- Gleichwertigkeitstest `tools/rust_parity.py`: Python und Rust liefern für 3000 Zufallsfälle identische Ergebnisse.

### Behoben (Python)
- Meldung „Sehr große Datei“: Das Komma im Satz wurde fälschlich durch einen Punkt ersetzt („215 Bytes. Grenze …“). Gefunden durch den Gleichwertigkeitstest.

## [0.7.0] – 2026-11-26

**Nur gegen den Simulator getestet.**

### Hinzugefügt
- **Steuerungsmenüs im Simulator nachgebildet:** Menüstruktur der DIALOG 11 mit Hauptbetriebsarten 1/2/3, Unterbetriebsarten (z. B. 21 TESTLAUF › 211 › 2117 START/STOP, 38 › 387 DATEN-TRANSFER), Hilfsbetriebsarten MODE, INFO, HELP (Menüstruktur), EDIT (Suchen/Ersetzen/Kopieren/Löschen/Schieben), GRAF und DIA (Dialogeingabe von Wegen und Zyklen). Dazu Bedienfeld ZYKLUS-START/STOP und EINZELSATZ, Menüpfad in der Betriebsartenanzeige, PRESET, FAHREN, SCHNITTDATEN-Rechner, Programmanwahl mit Satzvorlauf und Notizen. Nicht nachgebildete Maschinenfunktionen erscheinen grau mit Erklärung.
- **Programm- und Werkzeugverwaltung** (Seite 9): Programme, Makros und Werkzeugdateien mit Versionen, Wiederherstellen, Papierkorb, Programmnummer, Werkzeugprüfung und „zuletzt gesendet“; Bearbeiten im Simulator mit Speichern als neue Version; Werkzeugtabelle (speist den Simulator) mit Sicherung; Werkzeugdatei aus Vorlage (als unverifiziert gekennzeichnet).
- **Senden und Nachladebetrieb (LSV2) direkt aus der Programmverwaltung** – weiterhin mit Prüfung und Bestätigung; bei Werkzeugdateien zusätzliche Hinweise im Bestätigungsdialog.
- **Übertragung mitverfolgen:** Ein geöffnetes Simulatorfenster zeigt beim Senden/Nachladen den übertragenen Stand (ausdrücklich nicht die Maschinenposition).
- Dokumentation `docs/LIBRARY.md`, `docs/SIMULATOR.md` erweitert.

## [0.6.0] – 2026-11-25

**Der Simulator ist eine reine PC-Darstellung ohne Maschinenverbindung.** Seine Sprachbasis sind die DECKEL-Trainingsunterlagen zur DIALOG 11 (Module 3–14, nicht im Repository).

### Hinzugefügt
- **DIALOG-11-Simulator (Grafik, offline)** unter *Werkzeuge* (Strg+G): Interpreter für die DIALOG-11-Programmiersprache mit Grafik (XY, XZ, YZ, 3D) und einer Bedienung im Stil der Steuerung: Betriebsarten 1/2/3, Softkeys F1–F8, Start/Einzelsatz/Halt, Zeitleiste, Tempo 1×–1000×, Istwert/Restweg, Modalliste, Meldungen, Werkzeug-/Nullpunkt-/Parametertabellen, Befehlsübersicht und Demoprogramm.
- Ausgewertet werden `G0` (Knickbahn, `G0.1`, `G0.2`), `G1`, `G2`/`G3` (Mittelpunkt, Radius, `G9` polar, Vollkreis, Schraubenlinie), Polarkoordinaten `PX PY PW PL`, `G17`–`G19`, `G40`–`G42` mit `G45`–`G47`, `G51`/`G53`–`G56` mit Drehung, `G58`, `G59`, Zyklen `G81`–`G86`, `G87.1`–`G87.5`, `G87.9`, `G89.1`/`G89.2`, Unterprogramme/Makros, `L`-Wiederholungen und -Sprünge, Parameter mit Rechenfunktionen, `P..!` und `IF … THEN … ELSE`.
- Bearbeitete Programme lassen sich nur als **neue** Datei speichern; die Originaldatei bleibt unverändert.
- Dokumentation `docs/SIMULATOR.md` mit allen Vereinfachungen. `docs/DIALOG11.md` ergänzt um die Angaben aus Modul 14 (KONFIG I/O mit bis zu 19200 Baud, DATEN-TRANSFER, Parallel-, Nachlade- und DNC-Betrieb).

## [0.5.1] – 2026-11-20

**Nur gegen den Simulator getestet.** Grundlage der Änderungen ist eine echte Parameterausgabe einer FP2A (nicht im Repository enthalten).

### Behoben
- **EOT am Dateiende:** Von der DIALOG 11 ausgegebene Dateien enden mit `EOT` (0x04). Bisher wurde das als unzulässiges Steuerzeichen gemeldet und das Senden gesperrt. Jetzt Hinweis statt Fehler; `EOT` wird standardmäßig mitgesendet (`transfer.send_eot_trailer`). Bei LSV2 ersetzt das Protokoll-`EOT` das Datei-`EOT`.
- **Empfang endet bei EOT** (`receive.end_on_eot`), auch ohne `M30`; ein direkt auf `M30` folgendes `EOT` wird mit übernommen, andere Daten danach weiterhin nicht.

### Hinzugefügt
- **`M2` als zusätzliches Programmende** (Profil `M30,M2`; mehrere Alternativen kommagetrennt, `M2` erkennt auch `M02`).
- **Parameterdatei-Struktur:** Erkennung über Kopfgruppe `%MPG00`/`%MDG00 (KOPF)`; Prüfung von Gruppen, Zeilenformat, Gruppenende `?` und Prüfsummenzeile. Anzeige von Format, Softwarestand, Maschinentyp und -nummer.
- **Schreibsperre bei geänderter Gruppe mit alter Prüfsumme** (Prüfsummenverfahren unbekannt, wird nicht berechnet).
- **Schreibsperre bei abweichendem Softwarestand, Maschinennummer oder Format** gegenüber der aktuellen Sicherung.
- Simulator-Parameter im beobachteten Format (eindeutig als `SIMULATION` gekennzeichnet).

## [0.5.0] – 2026-11-19

**Nur gegen den Simulator getestet.**

### Hinzugefügt
- **Datenformat ermitteln (nur Mitlesen):** Parität/Datenbits werden aus Daten der CNC geschätzt (Port als 8N1, es wird nichts gesendet); Ergebnis als Profilkopie übernehmbar. Mitschnitte werden in `raw/` abgelegt.
- **Aufzeichnung abspielen:** Simulator-Szenario, das echte Rohdaten (z. B. nach dem ersten Empfang) wiedergibt.
- **Inbetriebnahme-Assistent:** Schritte der Inbetriebnahme mit Status/Notiz je Profil, Protokoll-Export (Markdown), „als verifiziert markieren" erst nach allen Pflichtschritten und mit Prüfername (Standardprofile bleiben unverändert). Echte Loopback-/Empfangs-/Format-Ergebnisse haken die Schritte automatisch ab (nie im Simulator).
- **Profile exportieren/importieren** (JSON, mit Prüfung; ID-Konflikte erhalten neue ID).
- **Menüleiste:** Datei (Öffnen, zuletzt geöffnet, Beenden), Ansicht (Schriftgröße), Werkzeuge, Hilfe (eingebautes Handbuch F1, Über).
- Eingebautes Handbuch: alle Dokumente werden mit Programm und Installer ausgeliefert.
- NC-Dateien per Drag & Drop oder als Startparameter laden (es wird nie automatisch gesendet); optional „Öffnen mit" für `.nc` im Installer.
- Schriftgröße 80–200 % (Strg + / Strg - / Strg 0).

### Behoben
- Fenster passt auf kleine Bildschirme (Mindestbreite vorher ca. 1350 Pixel); Seiten scrollen bei Bedarf.
- Unerwartete Programmfehler werden jetzt als Meldung angezeigt (zuvor nur im Log).
- Nur noch eine DeckelLink-Instanz pro Datenordner (verhindert Konflikte am Anschluss).
- Installer enthielt README/Lizenz/CHANGELOG nicht.

### Geändert
- GitHub-Actions auf aktuelle Versionen (checkout v7, setup-python v7, upload-artifact v7, download-artifact v8, action-gh-release v3).

## [0.4.1] – 2026-11-18

### Behoben
- Darstellung bei dunklem Windows-Farbschema: Beschriftungen und Knöpfe waren unsichtbar (helle Schrift auf hellem Grund), Listen und Textfelder schwarz. DeckelLink verwendet jetzt unabhängig vom System-Farbschema ein festes helles Design (Qt-Stil „Fusion" mit eigener Farbpalette).

## [0.4.0] – 2026-11-18

Erster veröffentlichter Stand (Vorabversion). **Nur gegen den Simulator getestet – keine reale DIALOG 11 geprüft.**

### Hinzugefügt
- Windows-Installer (Inno Setup): Installation ohne Administratorrechte, Startmenü-Einträge (normal und Simulationsmodus), optionales Desktop-Symbol, Deinstallation ohne Löschen der Benutzerdaten; portable ZIP-Version und SHA-256-Prüfsummen.
- Unterstützung für RS-232/Ethernet-Adapter: Verbindungsart TCP-Rohdaten (TCP-Server-Modus) und RFC 2217, wählbar in den Einstellungen; virtuelle COM-Ports von Hersteller-Treibern weiterhin als COM-Port.
- Verständliche Fehlermeldungen für nicht erreichbare Adapter und unpassende Einstellungen (CTS-Prüfung bei TCP-Rohdaten).
- Tests gegen einen lokalen TCP-Testserver (Senden/Empfangen, auch über die GUI).

### Bewusst nicht umgesetzt
- Fernsteuerung der Maschine (Programmstart, Achsbewegung, Betriebsartenwahl, Befehls-Terminal) – siehe `docs/SAFETY.md`.

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
