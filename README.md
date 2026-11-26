# DeckelLink

**NC-Programmübertragung zwischen einem Windows-PC und einer DECKEL FP2A mit DIALOG-11-Steuerung über V.24/RS-232.**

DeckelLink ist eine lokale Desktop-Anwendung (Python, PySide6) zum Senden, Empfangen, Prüfen und Archivieren von NC-Programmen. Ein eingebauter Simulator erlaubt die vollständige Bedienung ohne Maschine.

> **Verifikationsstatus:** Die Kommunikationsparameter der DIALOG 11 sind **nicht** an einer realen Maschine verifiziert. Beide mitgelieferten Profile sind Annahmen aus widersprüchlichen öffentlichen Angaben. Alle Tests dieses Projekts liefen ausschließlich gegen den Simulator. Siehe [docs/DIALOG11.md](docs/DIALOG11.md) und [docs/VERIFICATION_CHECKLIST.md](docs/VERIFICATION_CHECKLIST.md).

---

## Sicherheitsgrenzen

DeckelLink **überträgt ausschließlich Daten**. Die Software

- startet keinen CNC-Zyklus und keine Programmausführung,
- bewegt keine Achsen und schaltet keine Maschinenmodi um,
- sendet nichts automatisch (weder beim Programmstart noch nach Dateiauswahl),
- verändert niemals Originaldateien und überschreibt keine Dateien stillschweigend,
- liefert im Nachladebetrieb nur Daten, die die CNC anfordert – der Programmstart bleibt an der CNC,
- prüft **nicht**, ob ein Programm geometrisch, technologisch oder maschinensicher ist.

Programmauswahl, Freigabe und Ausführung bleiben vollständig an der CNC. Details: [docs/SAFETY.md](docs/SAFETY.md).

## LSV2 und NC-Nachladebetrieb

Mit einem LSV2-Profil (mitgeliefert: *DIALOG 11 – LSV2 4800 7E1 (Nachladebetrieb)*, nicht verifiziert) überträgt DeckelLink telegrammgesichert nach dem DIN-66019-Basisverfahren (ENQ, `DLE STX … DLE ETX` + BCC, Quittungen `DLE 0/1`, NAK-Wiederholung, EOT):

- **Senden / Empfangen** wie gewohnt, nur gesichert über Telegramme.
- **Nachladebetrieb (LSV2) …** auf der Seite *Übertragen*: Die CNC fordert die Sätze während der Bearbeitung durch ihre Quittungen an. Der Programmstart erfolgt ausschließlich an der CNC. Vor dem Start erscheint ein eigener Warnhinweis mit Pflichtbestätigung; während des Nachladebetriebs wird der Windows-Standby unterdrückt.

Rahmung, BCC-Bildung, Telegramminhalt, Start-/Endtelegramme und Timeouts sind im Profil (Reiter *Protokoll (LSV2)*) einstellbar, weil die genaue LSV2-Umsetzung der DIALOG 11 nicht verifiziert ist ([docs/DIALOG11.md](docs/DIALOG11.md)). Simulator-Szenarien `LSV2: …` testen Nachladebetrieb, NAK, verlorene Quittungen, WACK, CNC-Abbruch und Verbindungsabbruch.

## RS-232/Ethernet-Adapter

Unter *Einstellungen → Verbindung* wählbar: lokaler/virtueller **COM-Port**, **TCP-Rohdaten** (Adapter im TCP-Server-Modus) oder **RFC 2217** (Baudrate und Steuerleitungen werden übertragen). Alle Funktionen (Senden, Empfangen, Nachladebetrieb, Parameter, Diagnose, Loopback) laufen über den Adapter. Details und Einschränkungen: [docs/CABLE.md](docs/CABLE.md).

## Maschinenparameter

Seite *Maschinenparameter*: Parameter auslesen (der Bediener startet die Ausgabe an der CNC) und als unveränderliche, versionierte Sicherung mit SHA-256 ablegen, Parametersätze vergleichen (Diff) und – mehrfach abgesichert – eine Parameterdatei in die an der CNC freigegebene Parametereingabe schreiben. Schreiben ist standardmäßig gesperrt und erfordert Profilfreigabe, Freischaltung pro Sitzung, eine aktuelle Sicherung, einen Diff sowie die getippte Bestätigung `PARAMETER SCHREIBEN`; danach wird durch erneutes Auslesen kontrolliert. Es werden keine Fernbefehle gesendet; das Parameterformat der DIALOG 11 ist nicht verifiziert. Details: [docs/PARAMETERS.md](docs/PARAMETERS.md).

## Unterstützte Betriebssysteme

| System | Status |
|---|---|
| Windows 10 / 11, 64 Bit | Zielplattform (portable Anwendung, keine Administratorrechte nötig*) |
| Linux | Entwicklung und CI (Tests, Simulator) |

\* sofern der Treiber des USB-RS-232-Adapters den Zugriff ohne Administratorrechte erlaubt.

## Installation für Entwickler

Voraussetzung: Python 3.12 oder neuer.

```bash
git clone <repository-url> deckellink
cd deckellink
python -m venv .venv
# Windows:  .venv\Scripts\activate
# Linux:    source .venv/bin/activate
python -m pip install -e ".[dev]"
```

Weitere Hinweise (Windows-Build, Linux-Bibliotheken für Qt): [docs/INSTALLATION.md](docs/INSTALLATION.md).

## Starten

```bash
python -m deckellink            # normale Sitzung (startet beim ersten Mal im Simulationsmodus)
python -m deckellink --simulate # erzwingt Simulationsmodus für diese Sitzung
python -m deckellink --data-dir D:\DeckelLink-Daten   # portabler Datenordner
python -m deckellink --smoke-test   # headless Selbsttest im Simulator, Exit-Code 0 = OK
```

Benutzerdaten (Einstellungen, Logs, empfangene Programme, Backups) liegen in den benutzerspezifischen Verzeichnissen von `platformdirs`, unter Windows z. B. `%LOCALAPPDATA%\DeckelLink\`, niemals im Programmverzeichnis.

## Simulationsmodus

Beim ersten Start ist der **Simulationsmodus aktiv** – deutlich erkennbar am gelb-schwarz umrandeten Banner „SIMULATIONSMODUS – keine Verbindung zur Maschine". Der Realbetrieb wird unter *Einstellungen → Verbindung* bewusst gewählt und dann rot als „REALBETRIEB" angezeigt.

Der Simulator ersetzt nur die Transportschicht; Prüfung, Aufbereitung, Sende- und Empfangslogik sind identisch mit dem Realbetrieb. Vordefinierte Szenarien (Einstellungen → Simulator-Szenario):

| Szenario | Verhalten |
|---|---|
| Erfolgreiches Senden / Empfangen | Normalfall |
| CTS bleibt aus | Handshake-Timeout bei Profilen mit CTS-Prüfung |
| Timeout während des Sendens | CNC nimmt nach 100 Bytes nichts mehr an |
| Abbruch durch Benutzer | Langsame Übertragung zum manuellen Abbrechen |
| Empfang ohne M30 | Ende per Inaktivität, Kennzeichnung „unvollständig" |
| Empfang mit Nullbytes | Nullbytes im Rohstrom |
| Verbindungsabbruch | Abbruch nach 150 Bytes |
| Datei mit ungültigen ASCII-Zeichen | Prüfung sperrt die Übertragung |
| Langsame Übertragung | 1/5 Baudrate, kleine Pakete |
| Partielle Reads/Writes, gemischte Zeilenenden, beschädigte Bytes, Port belegt, Schreibfehler | weitere Fehlerbilder |

Mit **„Beispielprogramm (Simulator)"** auf der Seite *Übertragen* wird das Beispielprogramm des Szenarios als neue Datei angelegt und geladen.

## NC-Programm senden

1. *Übertragen* → **Datei öffnen** (Strg+O). Die Originalbytes werden geladen, nicht verändert.
2. Die Datei wird automatisch **geprüft**: Befunde als Info / Warnung / Fehler.
   - **Fehler** sperren die Übertragung (z. B. Umlaute, Steuerzeichen, leere Datei).
   - **Warnungen** (z. B. fehlendes M30) müssen per Checkbox bestätigt werden.
3. Die **Transfer-Vorschau** zeigt die aufbereiteten Daten (separate Kopie, z. B. CRLF-normalisiert).
4. **An Maschine senden …** öffnet die Bestätigung mit Datei, Profil, Port, Datenformat. Beim ersten realen Transfer erscheint zusätzlich der Sicherheitshinweis.
5. Während der Übertragung: Fortschritt, Zeile x von y, Bytes, Dauer, Rate, CTS/DSR/RTS/DTR. **Abbrechen** (oder Esc) stoppt sofort kontrolliert.

M30 wird **nie** automatisch ergänzt. Nur wenn das Profil es erlaubt *und* Sie es pro Übertragung ausdrücklich ankreuzen. M82 wird nur gesendet, wenn es im Profil als Startsequenz eingetragen ist.

## NC-Programm empfangen

1. *Empfangen* → **Empfang starten**. Der Port wird geöffnet, Status „Empfangsbereit – warte auf Daten der CNC".
2. Ausgabe an der CNC starten.
3. Live-Vorschau in ASCII und HEX. Das Ende wird erkannt durch die Endsequenz (Standard `M30` + Zeilenende) oder Inaktivität (Standard 5 s) – konfigurierbar.
4. Ergebnis: normalisierte NC-Datei in `received/` (bzw. `failed/` bei unvollständigem Empfang), Rohdaten in `raw/`, Metadaten als JSON (Zeit, Profil, Port, Parameter, Bytezahlen, SHA-256 roh/normalisiert, Abschlussgrund, Befunde).

Dateinamen: `RECEIVED_JJJJMMTT_HHMMSS.nc`, bei Konflikten `_001`, `_002` … Vorhandene Dateien werden nie überschrieben.

## Profile verwalten

*Einstellungen → Kommunikationsprofil*. Alle Parameter sind editierbar (Baudrate, Datenbits, Parität, Stoppbits, RTS/CTS, XON/XOFF, DSR/DTR, initiales RTS/DTR, CTS/DSR-Pflicht, Timeouts, Verzögerungen, Paketgröße, Zeilenende, Start-/Endsequenz, Empfangsende u. v. m.).

- Mitgelieferte Profile: `DIALOG 11 – 4800 7N1 RTS/CTS`, `DIALOG 11 – 4800 7E1 ohne Handshake` und `DIALOG 11 – LSV2 4800 7E1 (Nachladebetrieb)` (alle *nicht verifiziert*).
- Änderungen an Standardprofilen werden **als Kopie** gespeichert, das Original bleibt unverändert.
- Ungültige Profildateien führen nicht zum Absturz; die fehlerhaften Felder werden angezeigt, es wird auf ein sicheres Standardprofil zurückgefallen.
- Unsichere Kombinationen (z. B. unbegrenzter Empfang, sehr kurze Timeouts, aktiviertes M30-Ergänzen, unbekannte Startsequenz) werden als Warnung angezeigt.

## DIALOG-11-Simulator (Grafik, offline)

**Werkzeuge → DIALOG-11-Simulator** (Strg+G) rechnet ein NC-Programm in der DIALOG-11-Sprache am PC durch und zeigt die Bahnen in Draufsicht, Vorder- und Seitenansicht sowie 3D. Zur Bedienung gehören die Betriebsarten Handfahren/Automatik/Programm, Softkeys F1–F8, Einzelsatz, Istwert- und Restweganzeige, Modalliste und Laufzeitschätzung. Ausgewertet werden Wegbefehle, Bahnkorrektur, Nullpunkte, Drehen/Spiegeln/Skalieren, Bohr-, Taschen- und Musterzyklen, Unterprogramme, Wiederholungen und Parameterprogrammierung. Die Menüs der Steuerung (Haupt-, Unter- und Hilfsbetriebsarten mit Softkeys) sind nachgebildet. Der Simulator hat keine Verbindung zur Maschine. Details und Vereinfachungen: [docs/SIMULATOR.md](docs/SIMULATOR.md).

## Programm- und Werkzeugverwaltung

Die Seite **9 Programmverwaltung** verwaltet Programme, Makros und Werkzeugdateien mit Versionen, Papierkorb, Werkzeugprüfung und „zuletzt gesendet“. Die Werkzeugtabelle speist den Simulator. Aus der Verwaltung lässt sich direkt **zum Senden laden** oder der **Nachladebetrieb (LSV2)** starten – immer mit der üblichen Bestätigung. Ein Werkzeugdatei-Format der DIALOG 11 ist nicht bekannt: Von der Steuerung ausgelesene Werkzeugdateien können unverändert zurückgesendet werden, eigene Vorlagen sind als unverifiziert gekennzeichnet. Details: [docs/LIBRARY.md](docs/LIBRARY.md).

## Inbetriebnahme-Werkzeuge

- **Werkzeuge → Inbetriebnahme-Assistent:** führt durch die Schritte aus [docs/COMMISSIONING.md](docs/COMMISSIONING.md), dokumentiert Status und Notizen, exportiert ein Protokoll und markiert das Profil erst nach allen Pflichtschritten als verifiziert.
- **Werkzeuge → Datenformat ermitteln:** DeckelLink liest nur mit (8N1) und schätzt aus den Daten der CNC Parität und Datenbits – klärt z. B. 7N1 vs. 7E1, ohne etwas zu senden.
- **Simulator „Aufzeichnung abspielen":** spielt echte Rohdaten aus `raw/` ab, um am Schreibtisch mit dem realen Verhalten weiterzutesten.
- **Profile exportieren/importieren:** geprüfte Profile weitergeben.

## Bedienung

Menü *Datei* (Öffnen, zuletzt geöffnete Dateien), Drag & Drop von NC-Dateien auf das Fenster, *Ansicht* für größere Schrift (Strg +/–), *Hilfe → Handbuch* (F1) mit allen Dokumenten. Es läuft immer nur eine Instanz.

## Diagnose

- *Terminal*: TX/RX-Verkehr in ASCII, HEX oder kombiniert, begrenzte Historie, optionales Rohdatenprotokoll.
- *Diagnose*: Port, Profil, Baudrate, Datenrahmen, Flow Control, CTS/DSR/DCD/RI/RTS/DTR, Byte-Zähler, letzter Fehler, letzter Zustandswechsel. Port kann zur reinen Beobachtung geöffnet werden.
- **Loopback-Test** (TX/RX gebrückt): Testmuster, Vergleich, Dauer, Ergebnis bestanden / teilweise / fehlgeschlagen mit Ursachenhinweisen.
- **Diagnosepaket** (ZIP): Logs, anonymisierte Einstellungen, Profil, Systeminfo, letztes Ergebnis; NC-Dateien/Rohdaten nur auf ausdrücklichen Wunsch; Pfade optional maskiert.

## Tests

```bash
# Linux headless: QT_QPA_PLATFORM=offscreen
ruff check src tests
ruff format --check src tests
mypy
pytest --cov=deckellink
```

Die Tests verwenden keine echten COM-Ports. Aktueller Stand: 416 Tests, Gesamtabdeckung ca. 91 % (Mindestgrenze 80 %).

## Installation (Windows)

Installer `DeckelLink-<version>-Setup-x64.exe` von der GitHub-Release-Seite herunterladen und ausführen – keine Administratorrechte nötig. Alternativ die portable ZIP-Version. Details: [docs/INSTALLATION.md](docs/INSTALLATION.md).

## Windows-Build

```bash
python -m pip install -e ".[dev,build]"
pyinstaller --noconfirm --clean deckellink.spec
dist\DeckelLink\DeckelLink.exe --smoke-test
```

Ergebnis ist ein **portabler Onedir-Ordner** `dist/DeckelLink/` (ohne Python-Installation lauffähig). GitHub Actions (`build-windows.yml`) baut das ZIP-Artefakt automatisch, bei Tags `v*` als Release-Entwurf.

## Weitere Dokumentation

| Dokument | Inhalt |
|---|---|
| [ARCHITECTURE.md](docs/ARCHITECTURE.md) | Schichten, Threads, Abweichungen von der Vorgabestruktur |
| [DIALOG11.md](docs/DIALOG11.md) | Bekannte/angenommene Steuerungseigenschaften |
| [CABLE.md](docs/CABLE.md) | RS-232-Grundlagen, Kabel, Adapter |
| [COMMISSIONING.md](docs/COMMISSIONING.md) | Sichere Inbetriebnahme Schritt für Schritt |
| [VERIFICATION_CHECKLIST.md](docs/VERIFICATION_CHECKLIST.md) | An der realen FP2A zu prüfende Punkte |
| [TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) | Fehlerbilder und Abhilfe |
| [SAFETY.md](docs/SAFETY.md) | Sicherheitskonzept |
| [PARAMETERS.md](docs/PARAMETERS.md) | Maschinenparameter auslesen, vergleichen, schreiben |
| [INSTALLATION.md](docs/INSTALLATION.md) | Installation, Build, Datenverzeichnisse |

## Lizenz

MIT – siehe [LICENSE](LICENSE). DECKEL und DIALOG sind Marken ihrer jeweiligen Inhaber; dieses Projekt steht in keiner Verbindung zu ihnen.
