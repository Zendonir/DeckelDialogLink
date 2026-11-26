# DeckelLink in Rust (Neubau, in Arbeit)

Ziel ist **maximale Programmstabilität**: eine einzelne Windows-`.exe` ohne Python-Laufzeit. Die Kernbibliothek kann nicht unbemerkt abstürzen. Die bisherige Python-Version bleibt nutzbar, bis die Rust-Version gleichwertig ist.

## Stabilitätsregeln (vom Compiler erzwungen)

- `unsafe` ist verboten (`forbid`).
- `unwrap`, `expect`, `panic!`, `todo!`, `unreachable!` und ungeprüfte Indexzugriffe (`a[i]`) sind im Produktionscode verboten (Clippy-Lints `deny`). Jeder Fehler wird als Wert behandelt und mit deutscher Meldung und möglichen Ursachen gemeldet.
- Ganzzahlüberläufe werden auch im Release-Build geprüft (`overflow-checks = true`).
- CI-Gates: `cargo fmt --check`, `cargo clippy -- -D warnings`, Tests unter Linux und Windows.
- **Eigenschaftstests (proptest):** Zufällige Bytes, Profile und Programme dürfen nie zu Abstürzen führen. Senden und Empfangen über die simulierte CNC liefert bitgenau dieselben Daten.
- **Gleichwertigkeitstest mit Python:** `tools/rust_parity.py` vergleicht Prüfung und Aufbereitung beider Versionen über Tausende Zufallsdateien und Profilvarianten. Dieser Test hat bereits einen Meldungsfehler in der Python-Version gefunden: „215 Bytes. Grenze“ statt „215 Bytes, Grenze“.

## Aufbau

```
rust/
  crates/deckellink-core/   Bibliothek ohne GUI
    model.rs, profile.rs    Profile (gleiches JSON-Format; mitgelieferte Profile werden aus
                            src/deckellink/resources/profiles eingebunden - eine Quelle für beide)
    nc/                     Prüfung (analyzer), Aufbereitung (transformer), Bytehilfen (checks)
    clock.rs                Zeitquelle (System, beschleunigt, virtuell für Tests), Abbruch-Token
    transport/              serieller Port (serialport), TCP-Netzwerkadapter, Simulator, Loopback
    transfer/               Senden und Empfangen ohne Protokoll, Zustände, Fortschritt
    storage.rs              SHA-256, exklusives Anlegen, atomares Schreiben (nie still überschreiben)
  crates/deckellink-cli/    Kommandozeile
```

## Kommandozeile (Etappe 1)

```
deckellink-cli profiles
deckellink-cli check TEIL.nc [--profile dialog11_7e1_none]
deckellink-cli prepare TEIL.nc TEIL_transfer.nc
deckellink-cli ports
deckellink-cli send TEIL.nc --sim
deckellink-cli send TEIL.nc --port COM3 --profile dialog11_7n1_rtscts
deckellink-cli receive NEU.nc --port COM3
```

Senden an einen realen Port (`--port`, `--tcp`) beginnt erst, wenn unmittelbar vorher `SENDEN` eingetippt wurde. Dateien werden nie überschrieben.

## Bauen

```
cd rust
cargo test --all
cargo build --release      # target/release/deckellink-cli(.exe)
```

## Etappen

| Etappe | Inhalt | Stand |
|---|---|---|
| 1 | Profile, NC-Prüfung/-Aufbereitung, Ports (seriell, TCP), Simulator, Senden/Empfangen, CLI, CI, Gleichwertigkeitstest | **fertig** |
| 2 | LSV2-Protokoll und Nachladebetrieb, simulierte LSV2-CNC, RFC 2217 | offen |
| 3 | Maschinenparameter (Dateistruktur, Sicherung, Schreibsperren) | offen |
| 4 | Oberfläche (egui): Übertragen, Empfangen, Archiv, Einstellungen, Diagnose, Terminal, Protokoll | offen |
| 5 | DIALOG-11-Simulator mit Grafik und Steuerungsmenüs | offen |
| 6 | Programm- und Werkzeugverwaltung, Inbetriebnahme-Assistent, Windows-Installer | offen |

Bis Etappe 4 abgeschlossen ist, bleibt die Python-Version die Anwendung mit Oberfläche.
