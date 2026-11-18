# Verifikations-Checkliste DECKEL FP2A / DIALOG 11

Alle Punkte sind **offen**. DeckelLink wurde bisher ausschließlich gegen den Simulator getestet. Bitte Ergebnisse mit Datum, Maschinennummer, Steuerungs-/Softwarestand und Prüfer eintragen.

| # | Punkt | Annahme / Standard in DeckelLink | Profilfeld | Ergebnis an realer FP2A |
|---|---|---|---|---|
| 1 | Tatsächliche Baudrate | 4800 (beide Profile) | `serial.baud_rate` | offen |
| 2 | Datenbits | 7 | `serial.data_bits` | offen |
| 3 | Parität | Profil A: keine · Profil B: gerade | `serial.parity` | offen |
| 4 | Stoppbits | 1 | `serial.stop_bits` | offen |
| 5 | Handshake | A: RTS/CTS · B: keiner | `serial.rts_cts`, `xon_xoff`, `dsr_dtr` | offen |
| 6 | RTS/CTS-Verhalten (wann setzt die CNC CTS? hält sie den Sender an?) | A: CTS vor Sendebeginn erforderlich, max. 30 s Wartezeit | `serial.require_cts_before_send`, `handshake_timeout_ms`, `initial_rts` | offen |
| 7 | DTR/DSR-Verhalten | DTR initial aus, DSR nicht erforderlich | `serial.initial_dtr`, `require_dsr_before_send` | offen |
| 8 | Genaue Steckerart an der Maschine | unbekannt | – | offen |
| 9 | Kabelbelegung | unbekannt – keine Belegung vorgegeben | – | offen |
| 10 | Programmkopf (Zeichen vor dem Programm, z. B. `%`, Leader, Nullbytes) | keiner; Nullbytes werden entfernt | `transfer.program_start`, `strip_null_bytes` | offen |
| 11 | Verwendung von M82 | nicht verwendet (Startsequenz leer) | `transfer.program_start` | offen |
| 12 | Programmende | `M30` | `transfer.program_end`, `receive.end_sequence` | offen |
| 13 | Notwendige Zeichen nach M30 (CRLF? weitere?) | CRLF; Nachlaufzeit 500 ms beim Empfang | `transfer.line_ending`, `receive.end_grace_ms` | offen |
| 14 | Maximale Zeilenlänge | Warnung ab 120 Zeichen | `transfer.max_line_length` | offen |
| 15 | Maximale Programmgröße | Warnung ab 500 000 Bytes; Empfangslimit 2 000 000 Bytes | `transfer.max_program_bytes`, `receive.max_receive_bytes` | offen |
| 16 | Dateinamen-/Programmnummernformat | nicht ausgewertet → `RECEIVED_<Zeit>.nc` | `receive.program_name_pattern` | offen |
| 17 | Verhalten bei Sonderzeichen (Tab, `;`, Kommentare in Klammern, Kleinbuchstaben) | Tabs → Leerzeichen; `;`-Zeilen und Kommentare werden beibehalten (Warnung) | `transfer.tab_handling`, `skip_semicolon_lines`, `comment_handling` | offen |
| 18 | Verhalten beim Empfang (Startbedienung an CNC, Pausen, Ende) | Ende per M30 + Zeilenende oder 5 s Inaktivität; Wartezeit auf erste Daten 120 s | `receive.*` | offen |
| 19 | Verhalten beim Senden (Einlesebereitschaft, notwendige Start-/Endverzögerung, Zeichenverzögerung) | 1000 ms Start-/Endverzögerung, keine Zeichen-/Paketverzögerung | `transfer.start_delay_ms`, `end_delay_ms`, `character_delay_ms`, `packet_*` | offen |
| 20 | Möglichkeit oder Unmöglichkeit von Drip-Feed | nicht implementiert | – | offen |
| 21 | Zeilenende, das die CNC beim Senden ausgibt | CRLF erwartet, andere werden normalisiert | `receive.normalize_line_endings` | offen |
| 22 | Kommentarzeichen der Steuerung | `(` … `)` | `transfer.comment_open/close` | offen |

## Protokoll der Verifikation

| Datum | Maschine / Steuerungsstand | Profil | Geprüfte Punkte | Ergebnis | Prüfer |
|---|---|---|---|---|---|
| – | – | – | – | – | – |
