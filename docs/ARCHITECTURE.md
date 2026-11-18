# Architektur

## Schichten

```
┌───────────────────────────── gui/ (PySide6, GUI-Thread) ─────────────────────────────┐
│ main_window.py (Koordination) · pages/ · widgets/ · dialogs/ · viewmodels.py         │
│ workers.py: QThread + TaskWorker  ──Signale (queued)──►  Widgets                       │
└───────────────┬──────────────────────────────────────────────────────────────────────┘
                │ startet Jobs im Hintergrund-Thread
┌───────────────▼──────────────┐  ┌───────────────────────┐  ┌────────────────────────┐
│ transfer/                    │  │ nc/                   │  │ profiles/              │
│ sender.py   (SendState)      │  │ analyzer.py (lesend)  │  │ schema.py (Validierung)│
│ receiver.py (ReceiveState)   │  │ transformer.py (Kopie)│  │ loader.py, repository  │
│ cancellation.py, progress.py │  │ checks.py             │  └────────────────────────┘
└───────────────┬──────────────┘  └───────────────────────┘
                │ Transport-Protocol (base.py)
┌───────────────▼──────────────────────────────────────────┐  ┌────────────────────────┐
│ transport/                                               │  │ storage/               │
│ SerialTransport (pyserial) · SimulatedTransport ·        │  │ paths (platformdirs)   │
│ LoopbackTransport                                        │  │ archive, backup, files │
└──────────────────────────────────────────────────────────┘  │ checksums              │
┌──────────────────────────┐ ┌──────────────────────────┐     └────────────────────────┘
│ simulation/ scenarios,   │ │ diagnostics/ terminal,   │  domain/ (Modelle, Zustände,
│ runner (headless)        │ │ loopback_test, export,   │  Ergebnisse, Fehler) · clock.py
└──────────────────────────┘ │ error_messages           │  config.py · logging/setup.py
                             └──────────────────────────┘
```

| Schicht | Paket | Verantwortung |
|---|---|---|
| Domain & Konfigurationsmodelle | `domain/`, `config.py` | Unveränderliche Dataclasses: `Profile`, `SerialSettings`, `TransferSettings`, `ReceiveSettings`, Zustände, Ergebnisse, `UserFacingError` |
| NC-Validierung & Normalisierung | `nc/` | `analyze()` liest nur; `transform()` erzeugt separate Transferbytes |
| Protokoll- & Transferlogik | `transfer/` | Zustandsautomaten für Senden/Empfangen, kooperativer Abbruch, Timeouts, Fortschritt |
| Serielle Transportschicht | `transport/` | `Transport`-Protocol, pyserial-Implementierung, Fehlerklassifikation |
| Simulator | `transport/simulated_transport.py`, `simulation/` | Simulierte CNC, Szenarien, Headless-Runner/Smoke-Test |
| Archivierung & Backups | `storage/` | Benutzerverzeichnisse, eindeutige Namen, exklusives Schreiben, Backups, SHA-256 |
| Logging & Diagnose | `logging/`, `diagnostics/` | Rotierende Logs, Terminalmodell, Loopback-Test, Diagnosepaket, Fehlertexte |
| GUI | `gui/` | Seiten, Widgets, Dialoge, Worker |
| Anwendungskonfiguration | `config.py`, `app.py` | Einstellungen (JSON), Einstiegspunkt, CLI |

## Transport-Schnittstelle

`transport/base.py` definiert ein `typing.Protocol` mit `open()`, `close()`, `is_open`, `write(data) -> int` (darf partiell schreiben), `read(size) -> bytes` (liefert `b""` nach Timeout), `cancel()`, `get_modem_status()`, `set_control_lines()`, `bytes_waiting`, `out_waiting`, `reset_input_buffer()`. Fehler werden als `TransportError`-Unterklassen gemeldet (`PortBusyError`, `PortNotFoundError`, `TransportTimeoutError`, `HandshakeTimeoutError`, `ConnectionLostError`, …), nie als rohe pyserial-Exceptions.

`SerialTransport` erhält eine `serial_factory`, sodass die Parameterabbildung ohne echten Port getestet wird.

## Zeit und Nebenläufigkeit

- Sämtliche Zeitabfragen und Wartezeiten laufen über `clock.Clock`. Tests nutzen `FakeClock` (virtuelle Zeit, deterministisch, schnell); die GUI-Tests nutzen `ScaledClock` (Simulator läuft z. B. 200× schneller).
- Jeder Port-Vorgang (Senden, Empfangen, Loopback, Diagnose-Monitor) ist ein Job in `gui/workers.py`: eigener `QThread`, Worker-Objekt ohne Widget-Zugriff, Kommunikation ausschließlich über Qt-Signale (queued in den GUI-Thread).
- Abbruch über `CancellationToken` (thread-sicher). Das Token ruft zusätzlich `transport.cancel()` auf, damit blockierende pyserial-Aufrufe sofort zurückkehren.
- Am Ende von `TaskWorker.run()` wird der Worker in den GUI-Thread zurückgeschoben, damit er dort zerstört wird. `MainWindow.shutdown()` bricht laufende Jobs ab und wartet auf die Threads, bevor Widgets zerstört werden.
- Es läuft immer höchstens ein Port-Vorgang gleichzeitig (`MainWindow.busy`).

## Datenfluss Senden

1. `load_file()` liest die Originalbytes (`bytes`, unveränderlich).
2. `analyze()` → `AnalysisResult` (Befunde mit Zeilennummern, SHA-256 des Originals).
3. `transform()` → `TransformResult` (Transferbytes + Zeilen-Offsets für die Zeilenanzeige).
4. Freigabe-Logik `viewmodels.evaluate_send_gate()` (Datei, Fehler, Warnungsbestätigung, Port/Simulator, belegt).
5. Bestätigungsdialog (inkl. Sicherheitshinweis beim realen Transfer).
6. `ProgramSender.send()` im Worker: Port öffnen → ggf. auf CTS/DSR warten → Startverzögerung → Chunks schreiben (partielle Writes, Stall-Timeout) → Ausgangspuffer leeren → Endverzögerung → Port schließen (bei Fehler/Abbruch immer).

## Datenfluss Empfangen

`ProgramReceiver.receive()` → Rohbytes unverändert + Zeitstempel → `process_received()` (Nullbytes entfernen, Zeilenenden normalisieren, Analyse, Vollständigkeit) → `ReceiveArchive.store()` (exklusives Schreiben, `received/` oder `failed/`, Rohdaten in `raw/`, JSON-Metadaten).

## LSV2 und Nachladebetrieb

```
transfer/lsv2.py           Sicherungsschicht DIN 66019: encode_telegram, FrameDecoder (inkrementell),
                           Lsv2Link (establish / send_telegram / finish bzw. accept / receive_event)
transfer/lsv2_transfer.py  Lsv2ProgramSender (Speicherübertragung + Nachladebetrieb),
                           Lsv2ProgramReceiver; gleiche Result-/Callback-Typen wie Klartext
simulation/lsv2_cnc.py     reaktive simulierte LSV2-CNC (Empfänger mit Satzpuffer/Abarbeitung, Sender,
                           Fehlerinjektion: NAK, verlorene Quittung, WACK, EOT, Abbruch, nie bereit)
power.py                   verhindert Windows-Standby während des Nachladebetriebs
```

Das Profil enthält den optionalen Abschnitt `protocol` (`mode: plain|lsv2`). Fehlt er, gilt Klartext – bestehende Profile bleiben gültig. Die GUI wählt Sender/Empfänger anhand von `profile.protocol.mode`. Der Nachladebetrieb unterscheidet sich vom Senden nur durch das lange Quittungs-Timeout, den Zustand `DRIP_FEEDING`, die eigene Bestätigung und die Standby-Sperre; das Tempo bestimmt die CNC über ihre Quittungen.

## Netzwerkadapter

`transport/network_transport.py`: `NetworkSerialTransport` nutzt die URL-Handler von pyserial (`socket://` für TCP-Rohdaten, `rfc2217://`) und erbt die gesamte Fehlerbehandlung von `SerialTransport`. Bei TCP-Rohdaten werden Modemleitungen als „unbekannt" gemeldet (pyserial liefert dort Platzhalterwerte) und Profile mit CTS/DSR-Pflicht abgewiesen. `create_hardware_transport()` wählt anhand der Einstellung `connection_kind` den Transport; die GUI kennt nur noch den „Verbindungsziel"-Begriff (`connection_target`).

## Maschinenparameter

```
parameters/service.py   Parameter-Transfereinstellungen (inhaltsneutral), Analyse, Diff,
                        evaluate_parameter_write_gate (alle Schreibvoraussetzungen, reine Funktion)
parameters/transfer.py  read_parameters / write_parameters über Klartext- oder LSV2-Logik
parameters/archive.py   versionierte Ablage <Datenordner>/parameters (nie überschreiben)
gui/pages/parameters_page.py, gui/dialogs/parameter_write_dialog.py
```

Die Sitzungsfreischaltung liegt nur im Speicher von `MainWindow` (`param_unlocked`) und wird nie gespeichert. Nach einem Schreibvorgang wird die Sicherung verworfen; das nächste Auslesen wird automatisch mit den geschriebenen Daten verglichen.

## Abweichungen von der Vorgabestruktur

| Vorgabe | Umsetzung | Grund |
|---|---|---|
| `profiles/*.json` im Repository-Wurzelverzeichnis | `src/deckellink/resources/profiles/*.json` | Profile müssen als Paketdaten in Wheel und PyInstaller-Build enthalten sein; eine zweite Kopie im Wurzelverzeichnis würde auseinanderlaufen. `profiles/README.md` verweist darauf. |
| `gui/` ohne Unterordner für Seiten | zusätzlich `gui/pages/` | Eine Datei je Navigationsseite hält `main_window.py` überschaubar. |
| — | `simulation/` (scenarios, runner) | Szenarien und Headless-Runner werden von GUI, Tests und `--smoke-test` gemeinsam genutzt. |
| — | `clock.py` | Deterministische Zeit für Tests und Simulator. |
| — | `diagnostics/error_messages.py`, `diagnostics/loopback_test.py` | Fehlertexte und Loopback-Logik sind GUI-unabhängig testbar. |
| — | `storage/files.py` | Zentrale Regeln gegen stilles Überschreiben. |
| — | `config.py`, `safety.py` | Anwendungseinstellungen bzw. zentrale Sicherheitstexte. |

## Erweiterbarkeit

Neue Transporte (z. B. TCP-Seriell-Konverter) implementieren nur das `Transport`-Protocol. Neue Steuerungen erhalten eigene Profile. Drip-Feed ist bewusst **nicht** implementiert (siehe `DIALOG11.md`).
