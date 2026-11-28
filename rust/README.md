# DeckelLink in Rust (Neubau, in Arbeit)

Ziel ist **maximale Programmstabilität**: eine einzelne Windows-`.exe` ohne Python-Laufzeit. Die Kernbibliothek kann nicht unbemerkt abstürzen. Die bisherige Python-Version bleibt nutzbar, bis die Rust-Version gleichwertig ist.

## Stabilitätsregeln (vom Compiler erzwungen)

- `unsafe` ist verboten (`forbid`).
- `unwrap`, `expect`, `panic!`, `todo!`, `unreachable!` und ungeprüfte Indexzugriffe (`a[i]`) sind im Produktionscode verboten (Clippy-Lints `deny`). Jeder Fehler wird als Wert behandelt und mit deutscher Meldung und möglichen Ursachen gemeldet.
- Ganzzahlüberläufe werden auch im Release-Build geprüft (`overflow-checks = true`).
- CI-Gates: `cargo fmt --check`, `cargo clippy -- -D warnings`, Tests unter Linux und Windows.
- **Eigenschaftstests (proptest):** Zufällige Bytes, Profile und Programme dürfen nie zu Abstürzen führen. Senden und Empfangen über die simulierte CNC liefert bitgenau dieselben Daten.
- **Gleichwertigkeitstest mit Python:** `tools/rust_parity.py` vergleicht Prüfung und Aufbereitung beider Versionen über Tausende Zufallsdateien und Profilvarianten. Für Maschinenparameter vergleicht er Struktur, Befunde, Diff-Zählung und Diff-Text sowie die Schreibsperren. Dafür erzeugt er Tausende zufällige und veränderte Parameterdateien. Erkennung des Datenformats und Loopback-Prüfmuster prüft er ebenfalls mit Zufallsdaten. Für LSV2 vergleicht er außerdem 14 Szenarien Byte für Byte: gesendete und empfangene Bytes, Endzustand, Daten und Dauer. Die Szenarien decken NAK, verlorene Quittung, WACK, EOT durch die CNC, Verbindungsabbruch, BCC-Varianten, Start- und Endtelegramme sowie den Empfang mit fehlerhafter Prüfsumme ab. Dieser Test hat bereits einen Meldungsfehler in der Python-Version gefunden: „215 Bytes. Grenze“ statt „215 Bytes, Grenze“.

## Aufbau

```
rust/
  crates/deckellink-core/   Bibliothek ohne GUI
    model.rs, profile.rs    Profile (gleiches JSON-Format; mitgelieferte Profile werden aus
                            src/deckellink/resources/profiles eingebunden - eine Quelle für beide)
    nc/                     Prüfung (analyzer), Aufbereitung (transformer), Bytehilfen (checks)
    clock.rs                Zeitquelle (System, beschleunigt, virtuell für Tests), Abbruch-Token
    transport/              serieller Port (serialport), TCP-Netzwerkadapter, RFC 2217 (rfc2217.rs),
                            Simulator, simulierte LSV2-CNC (lsv2_sim.rs), Loopback
    transfer/               Senden und Empfangen ohne Protokoll, Zustände, Fortschritt;
                            LSV2-Verbindung und Telegramme (lsv2.rs), LSV2-Senden/-Nachladen/-Empfangen
                            (lsv2_transfer.rs)
    parameters/             Maschinenparameter: Dateistruktur (fileformat), Vergleich wie Python-difflib
                            (difflib), Schreibsperren (mod.rs), Lesen/Schreiben (transfer), Archiv (archive)
    storage.rs              SHA-256, exklusives Anlegen, atomares Schreiben (nie still überschreiben)
    appdata.rs              Datenordner, settings.json und Empfangsarchiv (kompatibel zur Python-Version)
    scenarios.rs            Simulationsszenarien (gleiche IDs und Texte wie Python)
    library.rs              Programmverwaltung (Versionen, Papierkorb; gleiches Ordnerformat wie Python)
    tooldb.rs               Werkzeugtabelle tools.json, Prüfung, Werkzeugdatei aus Vorlage (nicht verifiziert)
    sim/                    DIALOG-11-Simulator: Syntax, Rechenausdrücke, Geometrie, Interpreter,
                            Zyklen G81-G89, Wiedergabe, Menübaum, Befehlsübersicht
  crates/deckellink-cli/    Kommandozeile
  crates/deckellink-gui/    Oberfläche (egui), Programm deckellink(.exe)
    app.rs                  Zustand und Aktionen (ohne Fenster testbar)
    ui.rs                   Seiten und Dialoge
    worker.rs               Hintergrund-Threads für alle Port-Zugriffe
    logic.rs                Sperren (Gates), Anzeigehilfen, Transportauswahl
```

## Oberfläche (Etappe 4)

`deckellink` (unter Windows `deckellink.exe`) ist die Oberfläche in Rust. Sie hat dieselben Seiten wie die Python-Version: Übertragen, Empfangen, NC-Programme, Maschinenparameter, Terminal, Diagnose, Einstellungen und Protokoll.

Datenordner, `settings.json`, Empfangsarchiv und Parameterarchiv sind dieselben wie in der Python-Version. Beide Versionen können deshalb während der Übergangszeit nebeneinander benutzt werden.

Diese Sicherheitsregeln gelten wie in Python:
- Nach der Installation startet das Programm im Simulationsmodus.
- Simulation und Realbetrieb sind deutlich zu unterscheiden. Bei Simulation sind Banner und Startknopf gelb mit schwarzem Rahmen, im Realbetrieb dunkelrot. Der Wechsel in den Realbetrieb muss eigens bestätigt werden.
- Gesendet wird erst nach einem Bestätigungsdialog unmittelbar vor der Übertragung. Darin stehen Datei, Profil, Schnittstelle, Aufbereitung und bestätigte Warnungen. Im Realbetrieb kommt der Sicherheitshinweis dazu.
- Der Nachladebetrieb ist nur mit einem LSV2-Profil möglich und verlangt zusätzlich die Bestätigung „Maschine bei Störungen an der CNC anhalten“.
- Alle Port-Zugriffe laufen in einem Hintergrund-Thread. Die Oberfläche friert deshalb nie ein, und „Abbrechen“ wirkt sofort.
- Stürzt der Hintergrundvorgang ab, wird das als Fehler gemeldet. Das Programm selbst läuft weiter.
- Maschinenparameter: Es gelten alle Schreibsperren aus Etappe 3. Die Freischaltung gilt nur für die laufende Sitzung.

**Aufbau:** wie in der Python-Version, der Arbeitsbereich ist der Bedienung der DIALOG 11 nachempfunden. Die Gestaltung ist eine eigene, keine Kopie des Originalbildschirms.
- Oben Menüleiste (Datei, Ansicht, Werkzeuge, Hilfe) und Kopfleiste. Die Kopfleiste zeigt Maschine, Status, Schnittstelle, Profil sowie das Betriebsart-Banner: Simulation gelb mit schwarzem Strichrahmen, Realbetrieb dunkelrot.
- Links stehen die nummerierten Seitentasten 1–8 in der Reihenfolge der Python-Version, darunter die Hilfsfunktionen HELP und Inbetriebnahme.
- In der Mitte liegt der dunkle Bildschirm. Oben zeigt die gelbe Betriebsartenanzeige z. B. `1 ÜBERTRAGEN › BEREIT ZUM SENDEN`, darunter folgen umrahmte Anzeigebereiche in Monospace-Schrift.
- Unter dem Bildschirm liegen die Softkeys F1–F8 mit den Aktionen der jeweiligen Seite. Sie sind auch über die Tasten F1–F8 bedienbar.
  - Übertragungstasten sind in der Simulation gelb, im Realbetrieb dunkelrot.
  - Jede Übertragung führt weiterhin über den Bestätigungsdialog.
- Tastatur:
  - Esc bricht einen laufenden Vorgang ab.
  - Strg+1…8 wählt eine Seite.
  - Strg+O öffnet eine Datei.
  - Strg+Plus, Strg+Minus und Strg+0 ändern die Schriftgröße.
- Schriften: Inter und JetBrains Mono, eingebettet, siehe `crates/deckellink-gui/assets/fonts`.

Außerdem enthält die Oberfläche:
- **Profileditor:** Mitgelieferte Profile werden nie überschrieben; beim Speichern entsteht ein Benutzerprofil. Technische Änderungen setzen den Status auf „nicht verifiziert“. Kopieren, Löschen (mit Sicherung), Import und Export sind möglich.
- **Diagnose:**
  - Loopback-Test mit vier Prüfmustern.
  - Datenformat mitlesen: Es wird nur empfangen, nie gesendet. Die Auswertung schätzt Parität und Datenbits.
  - Diagnosepaket als ZIP mit maskierten Pfaden; NC-Dateien sind nur auf Wunsch dabei.
- **Simulationsszenario „Aufzeichnung abspielen“** mit echten Rohdaten.
- **Inbetriebnahme-Assistent:**
  - 14 Schritte, Stand je Profil, Protokoll als Markdown.
  - „Verifiziert“ lässt sich nur setzen, wenn alle Pflichtschritte erledigt sind und ein Prüfer eingetragen ist; dabei entsteht eine Profilkopie.
  - Ergebnisse aus dem Realbetrieb werden automatisch eingetragen.
- **Hilfe:** Die Dokumentation aus `README.md` und `docs/` ist ins Programm eingebaut.
- **Drag & Drop:** Eine Datei auf das Fenster ziehen lädt sie, gesendet wird nichts.

- **DIALOG-11-Simulator** (Taste „DIALOG-11-SIMULATOR“, Menü Werkzeuge oder Softkey „SIMULIEREN“ auf der Seite Übertragen):
  - Betriebsartentasten 1 HANDFAHREN, 2 AUTOMATIK, 3 PROGRAMM und Hilfsbetriebsarten MODE, INFO, HELP, EDIT, GRAF, DIA; die Softkeys F1–F8 folgen dem Menübaum der Steuerung.
  - Programmliste mit markiertem Satz, Werkzeugbahn in XY, XZ, YZ und 3D (Zoom, Verschieben, Drehen), Istwert und Restweg, Modalliste, Meldungen, Werkzeug- und Nullpunkttabelle, Parametertabelle, Befehlsübersicht.
  - Testlauf mit Zyklus-Start/-Stopp, Einzelsatz, Tempo und Zeitleiste; Handfahren und FAHREN nur im Simulator; Editieren mit DIA-Eingabe, Suchen, Ersetzen, Kopieren, Schieben.
  - Rein offline: kein Port, keine Übertragung. Die Originaldatei wird nie verändert; „Als neue Datei“ legt nur neue Dateien an.

- **Programmverwaltung** (Seitentaste 9):
  - Programme, Makros und Werkzeugdateien importieren; jede Änderung legt eine neue Version an, Löschen verschiebt in den Papierkorb (`library/deleted`).
  - Angaben (Name, Art, Nummer, Beschreibung), Vorschau, Werkzeugprüfung gegen die Werkzeugtabelle, frühere Versionen wiederherstellen.
  - „Zum Senden laden“ und „Nachladebetrieb“ laden nur auf die Seite Übertragen; gesendet wird erst nach der Bestätigung. Werkzeugdateien bekommen dort einen deutlichen Warnhinweis.
  - „Im Simulator ansehen/bearbeiten“; bearbeitete Programme werden als neue Version gespeichert.
- **Werkzeugtabelle** (`tools.json`, Sicherung vor jedem Speichern) für Simulator und Werkzeugprüfung; Werkzeugdatei nur aus einer selbst eingerichteten Vorlage (Format der DIALOG 11 nicht verifiziert).
- **Windows-Installer** `installer/deckellink-rust.iss` (siehe `docs/INSTALLATION.md`), Selbsttest `deckellink --smoke-test`.

Noch nicht in Rust: „Übertragung mitverfolgen“ im Simulator.

Tests:
- `tests/flows.rs` prüft die Abläufe ohne Fenster: Jedes Bild wird mit egui gezeichnet, die Aktionen sind dieselben wie bei den Schaltflächen.
- Geprüft werden Senden mit und ohne Bestätigung, Abbruch, Empfang ohne Überschreiben, Nachladebetrieb, alle Parameter-Schreibsperren und der Wechsel der Betriebsart.

Unter Linux braucht das Fenster zur Laufzeit `libxkbcommon-x11`. Unter Windows sind keine zusätzlichen Bibliotheken nötig.

## Kommandozeile (Etappen 1 bis 3)

```
deckellink-cli profiles
deckellink-cli check TEIL.nc [--profile dialog11_7e1_none]
deckellink-cli prepare TEIL.nc TEIL_transfer.nc
deckellink-cli ports
deckellink-cli send TEIL.nc --sim
deckellink-cli send TEIL.nc --port COM3 --profile dialog11_7n1_rtscts
deckellink-cli receive NEU.nc --port COM3
deckellink-cli send TEIL.nc --tcp 192.168.0.50:4001
deckellink-cli send TEIL.nc --rfc2217 192.168.0.50:4001
deckellink-cli send TEIL.nc --sim --profile dialog11_lsv2_7e1 --drip
deckellink-cli receive NEU.nc --sim --profile dialog11_lsv2_7e1
deckellink-cli param-check PARAM.par
deckellink-cli param-diff ALT.par NEU.par
deckellink-cli param-read --port COM3 --archiv D:\DeckelLink
deckellink-cli param-write NEU.par --port COM3 --freischalten --archiv D:\DeckelLink
```

Mit einem LSV2-Profil wird über LSV2 übertragen. `--drip` startet den Nachladebetrieb. Er ist nur mit LSV2-Profil möglich, und die CNC bestimmt das Tempo: Das nächste Telegramm geht erst nach ihrer Quittung hinaus. DeckelLink startet die Bearbeitung nie. Die LSV2-Einzelheiten bleiben unverifiziert, bis sie an der FP2A geprüft sind (siehe `docs/VERIFICATION_CHECKLIST.md`).

**Maschinenparameter** werden nur als Datei übertragen. Zum Auslesen startet man die Ausgabe an der CNC. Geschrieben wird nur in eine Parametereingabe, die an der CNC freigegeben ist. Fernbefehle gibt es nicht. `param-write` prüft dieselben Sperren wie die Python-Version, in dieser Reihenfolge:

1. Das Profil erlaubt das Schreiben (`parameters.allow_write`, standardmäßig aus).
2. Die Schreibfunktion ist für diesen Aufruf freigeschaltet: Option `--freischalten`, Sicherheitshinweis und Eingabe von `ja`.
3. Im selben Aufruf wird eine frische Sicherung ausgelesen und ins Archiv gelegt.
4. Die Datei ist fehlerfrei.
5. Der Vergleich mit der Sicherung wird angezeigt. Geschrieben wird nur, wenn Dateiformat, Softwarestand und Maschinennummer passen und sich bei jeder geänderten Gruppe auch die Prüfsummenzeile geändert hat.
6. Unmittelbar vor dem Senden muss `PARAMETER SCHREIBEN` eingetippt werden.
7. Danach werden die Parameter zur Kontrolle erneut ausgelesen und mit der geschriebenen Datei verglichen.

Das Parameterformat bleibt unverifiziert.

Senden an einen realen Port (`--port`, `--tcp`, `--rfc2217`) beginnt erst, wenn unmittelbar vorher `SENDEN` eingetippt wurde. Dateien werden nie überschrieben.

## Bauen

```
cd rust
cargo test --all
cargo build --release      # target/release/deckellink(.exe) und deckellink-cli(.exe)
```

## Etappen

| Etappe | Inhalt | Stand |
|---|---|---|
| 1 | Profile, NC-Prüfung/-Aufbereitung, Ports (seriell, TCP), Simulator, Senden/Empfangen, CLI, CI, Gleichwertigkeitstest | **fertig** |
| 2 | LSV2-Protokoll und Nachladebetrieb, simulierte LSV2-CNC, RFC 2217; Gleichwertigkeit auf Byte-Ebene (14 LSV2-Szenarien) | **fertig** |
| 3 | Maschinenparameter (Dateistruktur, Vergleich, Archiv, Schreibsperren, Kontrolle); Gleichwertigkeit für Zufalls-Parameterdateien inkl. Diff-Texten | **fertig** |
| 4 | Oberfläche (egui) mit allen Seiten, Profileditor, Diagnose (Loopback, Formaterkennung, Diagnosepaket), Aufzeichnung abspielen, Inbetriebnahme-Assistent, Hilfe | **fertig** |
| 5 | DIALOG-11-Simulator mit Grafik und Steuerungsmenüs; Gleichwertigkeit für das Demoprogramm und Zufallsprogramme (Bahnen, Meldungen, Zustände, Parameter) | **fertig** |
| 6 | Programm- und Werkzeugverwaltung, Windows-Installer mit Selbsttest; Gleichwertigkeit für Werkzeugaufrufe, Dateiart, Metadaten und Werkzeugdatei-Vorlage | **fertig** |

Mit Etappe 6 hat die Rust-Version den Funktionsumfang der Python-Version (bis auf „Übertragung mitverfolgen“ im Simulator). Bis zur Prüfung an der FP2A und der Freigabe durch den Anwender bleibt die Python-Version die Referenz.
