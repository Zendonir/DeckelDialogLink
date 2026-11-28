# Changelog

Format nach [Keep a Changelog](https://keepachangelog.com/de/1.1.0/), Versionierung nach [SemVer](https://semver.org/lang/de/).

## [Rust 0.8.0-alpha.8] – 2026-11-28

### Neu (Rust)
- **DIALOG-11-Simulator in Rust** (Etappe 5):
  - Kern `deckellink-core::sim`: Syntax, Rechenausdrücke, Geometrie, Interpreter mit Unterprogrammen, Parametern, IF/Sprüngen und Bahnkorrektur, Zyklen G81–G89, Wiedergabe, Menübaum und Befehlsübersicht.
  - Gleichwertigkeitstest gegen Python: Demoprogramm und Zufallsprogramme, verglichen werden Bahnen, Meldungen, Steuerungszustände und Parameter (0 Abweichungen).
- **Simulatorseite in der Oberfläche** im Aufbau der Steuerung:
  - Betriebsartentasten 1/2/3, Hilfsbetriebsarten MODE, INFO, HELP, EDIT, GRAF, DIA, Softkeys F1–F8 nach dem Menübaum.
  - Programmliste, Werkzeugbahn (XY, XZ, YZ, 3D), Istwert/Restweg, Modalliste, Meldungen, Werkzeug-, Nullpunkt- und Parametertabelle.
  - Testlauf mit Zyklus-Start/-Stopp und Einzelsatz, Handfahren, Editieren mit DIA, Suchen und Ersetzen.
  - Erreichbar über die Taste „DIALOG-11-SIMULATOR“, das Menü Werkzeuge und den Softkey „SIMULIEREN“ auf der Seite Übertragen.
  - Rein offline; Speichern nur als neue Datei.

### Behoben (Python und Rust)
- Simulator: Wurde die Punktgrenze beim Abschluss der Bahnkorrektur am Programmende erreicht, brach die Simulation mit einem Fehler ab. Jetzt endet sie mit „Abbruch: zu viele Bahnpunkte“.
- Simulator: Ein Parameter, der auf sich selbst verweist (z. B. `P8=(P8+1)` in der Parametertabelle), führte zu einer endlosen Rekursion. Jetzt erscheint die Meldung „Parameter-Verweise zu tief verschachtelt“.
- Simulator: Kehrt eine Kontur mit Bahnkorrektur exakt um (180°), hing die Eckform von der letzten Rundungsstelle ab. Jetzt wird die Spitze immer mit einem Übergangsbogen umfahren.
- Simulator-Menü: Hilfsbetriebsarten heißen in der Anzeige „EDIT“ statt „EDIT EDIT“.

## [Rust 0.8.0-alpha.7] – 2026-11-28

### Geändert (Rust-Oberfläche)
- **Aufbau wie in der Python-Version, Bedienung nach dem Vorbild der DIALOG 11:**
  - Menüleiste, Kopfleiste mit Betriebsart-Banner, nummerierte Seitentasten 1–8.
  - Dunkler Steuerungsbildschirm mit gelber Betriebsartenanzeige und Softkeys F1–F8 (auch über die F-Tasten).
  - Monospace-Anzeigen in umrahmten Bereichen.
  - Ersetzt das Karten-Design aus alpha.6.
- Tastatur: Esc bricht ab, Strg+1…8 wählt eine Seite, Strg+O öffnet eine Datei, Strg+Plus/Minus/0 ändert die Schriftgröße.
- Feste Farbgebung unabhängig vom Betriebssystem, wie in der Python-Version. Die Einstellung „Darstellung“ entfällt.
- Neuer Test: Bedienung über die F-Tasten. Eine Übertragung startet nie ohne Bestätigungsdialog.

## [Rust 0.8.0-alpha.6] – 2026-11-28

### Geändert (Rust-Oberfläche)
- **Neues Erscheinungsbild:**
  - Schriften Inter und JetBrains Mono (eingebettet, freie Lizenzen) und Material-Symbole.
  - Dunkles und helles Farbschema sowie „wie System“.
  - Kopfzeile mit Logo und Statusplaketten.
  - Gruppierte Navigation mit Symbolen.
  - Inhalte in Karten, hervorgehobene Hauptknöpfe, farbige Hinweisboxen, Terminal im Konsolenstil.
- Simulation (gelb) und Realbetrieb (dunkelrot) bleiben in beiden Farbschemata eindeutig unterscheidbar.

### Behoben (Rust)
- Die Einstellungen `simulation_replay_file` und `ui_theme` wurden beim Programmstart nicht gelesen. Folge: Die gewählte Aufzeichnung ging nach einem Neustart verloren.

## [Rust 0.8.0-alpha.5] – 2026-11-27

### Hinzugefügt (Rust, Etappe 4 abgeschlossen)
- **Profileditor:** alle Profilbereiche lassen sich bearbeiten.
  - Mitgelieferte Profile werden nie überschrieben; beim Speichern entsteht ein Benutzerprofil.
  - Technische Änderungen setzen den Status auf „nicht verifiziert“.
  - Kopieren, Löschen mit Sicherung, Import mit neuer ID bei Doppelung, Export.
- **Diagnose:**
  - Loopback-Test.
  - Datenformat mitlesen; es wird nur empfangen.
  - Diagnosepaket als ZIP mit maskierten Pfaden; NC-Dateien sind nur auf Wunsch dabei.
- **Simulationsszenario „Aufzeichnung abspielen“.**
- **Inbetriebnahme-Assistent:**
  - Dateiformat identisch zu Python, Protokoll als Markdown.
  - „Verifiziert“ nur, wenn alle Pflichtschritte erledigt sind und ein Prüfer eingetragen ist.
- **Hilfe:** Handbuch im Programm.
- **Drag & Drop:** lädt die Datei, sendet nie.
- **Gleichwertigkeitstest:** zusätzlich Formaterkennung und Loopback-Prüfmuster, 0 Abweichungen.

### Geändert
- Mindest-Rust-Version auf 1.95 angehoben (von den Bibliotheken vorgegeben).

## [Rust 0.8.0-alpha.4] – 2026-11-27

### Hinzugefügt (Rust, Etappe 4)
- **Oberfläche `deckellink(.exe)`** (egui) mit den Seiten Übertragen, Empfangen, NC-Programme, Maschinenparameter, Terminal, Diagnose, Einstellungen und Protokoll.
- **Sicherheitsregeln wie in Python:**
  - Start im Simulationsmodus.
  - Simulation gelb, Realbetrieb dunkelrot; der Wechsel in den Realbetrieb muss bestätigt werden.
  - Bestätigungsdialog unmittelbar vor jeder Übertragung.
  - Nachladebetrieb nur mit LSV2-Profil und eigener Bestätigung.
  - Parameter-Schreibsperren mit getippter Bestätigung und anschließender Kontrolle durch erneutes Auslesen.
- **Stabilität:** Port-Zugriffe laufen ausschließlich in Hintergrund-Threads. Bricht ein Hintergrundvorgang unerwartet ab, wird ein Fehler gemeldet; die Oberfläche läuft weiter.
- **Gemeinsame Daten mit der Python-Version:** Datenordner, `settings.json` (unbekannte Einträge bleiben erhalten), Empfangsarchiv mit denselben Metadaten, Parameterarchiv, die 23 Simulationsszenarien und importierte Profile.
- **Tests ohne Fenster:** echte egui-Bilder für alle Seiten und Dialoge sowie die Abläufe Senden, Abbruch, Empfang, Nachladebetrieb, Parameter schreiben und Betriebsartwechsel.
- **CI:** zusätzlich die Windows-Anwendung `deckellink.exe` als Artefakt.

### Noch nicht in der Rust-Oberfläche
- Profileditor, Loopback-Test, Erkennung des Datenformats, Export der Diagnose, Szenario „Aufzeichnung abspielen“, Inbetriebnahme-Assistent, Hilfe.
- DIALOG-11-Simulator und Programmverwaltung (Etappen 5 und 6).

## [Rust 0.8.0-alpha.3] – 2026-11-27

### Hinzugefügt (Rust, Etappe 3)
- **Maschinenparameter**: Die Dateistruktur wird wie in Python geprüft: Gruppen %MPGnn/%MDGnn, Gruppenende „?“, Prüfsummenzeile, Kopfgruppe mit Softwarestand und Maschinennummer, EOT. Das Prüfsummenverfahren bleibt unbekannt und wird nie berechnet.
- **Vergleich** zeilengenau mit denselben Ergebnissen wie Pythons `difflib`, einschließlich Diff-Text.
- **Archiv** für ausgelesene Parametersätze mit Metadaten und Rohdaten. Dateien werden nie überschrieben.
- **Schreibsperren** unverändert wie in Python: Profilfreigabe, Freischaltung nur für die Sitzung, frische Sicherung, fehlerfreie Datei, Vergleich, Format-, Softwarestand- und Prüfsummensperre, getippte Bestätigung `PARAMETER SCHREIBEN`, Kontrolle durch erneutes Auslesen.
- Lesen und Schreiben von Parametern ohne Protokoll und über LSV2, nie im Nachladebetrieb.
- Kommandozeile: `param-check`, `param-diff`, `param-read`, `param-write`.
- Gleichwertigkeitstest um zufällige und veränderte Parameterdateien erweitert, darunter lange Dateien mit gekürztem Diff. 0 Abweichungen.

## [Rust 0.8.0-alpha.2] – 2026-11-27

### Hinzugefügt (Rust, Etappe 2)
- **LSV2** (DIN 66019 Basic Mode): Verbindungsaufbau (ENQ/DLE 0), Telegramme mit DLE-Transparenz und konfigurierbarer BCC, abwechselnde Quittungen, NAK-Wiederholung, WACK, RVI und EOT. Telegrammaufbau, Zeitgrenzen und Wiederholungen kommen wie in Python aus dem Profil und bleiben unverifiziert.
- **Nachladebetrieb** über LSV2: Die CNC bestimmt das Tempo, das nächste Telegramm geht erst nach ihrer Quittung hinaus. Bei Abbruch wird, falls im Profil eingestellt, EOT gesendet.
- LSV2-Empfang mit Prüfsummenkontrolle und Größengrenze.
- Simulierte LSV2-CNC mit Fehlerbildern: NAK, verlorene Quittung, WACK, EOT, Trennung, nicht bereit, fehlerhafte BCC.
- **RFC 2217** (Telnet-COM-Port-Steuerung) für Netzwerkadapter: Baudrate und Datenformat werden am Adapter gesetzt, die Modemleitungen gemeldet. Unterstützt der Adapter RFC 2217 nicht, erscheint eine klare Meldung.
- Kommandozeile: `--drip` (Nachladebetrieb) und `--rfc2217 HOST:PORT`. Ein LSV2-Profil überträgt automatisch über LSV2.
- Gleichwertigkeitstest um 14 LSV2-Szenarien erweitert: Gesendete und empfangene Bytes, Zustand, Daten und Dauer stimmen mit Python überein. Zufallsfälle und LSV2-Szenarien ergeben 0 Abweichungen.

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
