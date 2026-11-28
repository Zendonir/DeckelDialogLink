# Verifikations-Checkliste DECKEL FP2A / DIALOG 11

Kein Punkt ist an der realen FP2A geprüft; DeckelLink wurde bisher ausschließlich gegen den Simulator getestet. Seit dem 28.11.2026 belegen die Steuerungshandbücher 2 und 3 (**HB2**/**HB3**, Softwarestand 4) und echte Ausgabedateien dieser Maschine viele Punkte; das steht in der Spalte „Laut Unterlagen“. Die Spalte „Ergebnis an realer FP2A“ bleibt für die Prüfung an der Maschine. Bitte Ergebnisse mit Datum, Maschinennummer, Steuerungs-/Softwarestand und Prüfer eintragen.

| # | Punkt | Annahme / Standard in DeckelLink | Profilfeld | Laut Unterlagen (HB2/HB3, echte Ausgaben) | Ergebnis an realer FP2A |
|---|---|---|---|---|---|
| 1 | Tatsächliche Baudrate | 4800 (beide Profile); laut Trainingshandbuch an der Steuerung bis 19200 einstellbar (KONFIG I/O) | `serial.baud_rate` | HB2: 75–19200 je Gerät einstellbar; Werkseinstellung nicht angegeben | offen (an der CNC unter KONFIG I/O ablesen) |
| 2 | Datenbits | 7 | `serial.data_bits` | HB2: 7 oder 8 einstellbar | offen |
| 3 | Parität | Profil A: keine · Profil B: gerade | `serial.parity` | HB2: Even, None oder Odd einstellbar | offen |
| 4 | Stoppbits | 1 | `serial.stop_bits` | HB2: 1 oder 2 einstellbar | offen |
| 5 | Handshake | A: RTS/CTS · B: keiner | `serial.rts_cts`, `xon_xoff`, `dsr_dtr` | Technische Daten: „XON/XOFF auch ohne Protokoll bzw. RTS/CTS“; Originalkabel kreuzt RTS/CTS | offen |
| 6 | RTS/CTS-Verhalten (wann setzt die CNC CTS? hält sie den Sender an?) | A: CTS vor Sendebeginn erforderlich, max. 30 s Wartezeit | `serial.require_cts_before_send`, `handshake_timeout_ms`, `initial_rts` | nicht beschrieben | offen |
| 7 | DTR/DSR-Verhalten | DTR initial aus, DSR nicht erforderlich | `serial.initial_dtr`, `require_dsr_before_send` | HB3 4-2: Steuerung führt kein DTR/DSR/DCD; am PC hängt DSR beim Originalkabel offen → nicht voraussetzen | offen |
| 8 | Genaue Steckerart an der Maschine | unbekannt | – | HB2 11-5, HB3 4-2: 25-polige Buchse unten am Bedienpult (Kanal 1, DTE); Kanal 2 Option; Kanal 3 Klinke (Kassette) | offen |
| 9 | Kabelbelegung | unbekannt – keine Belegung vorgegeben | – | HB3 4-3/4-4: Originalkabel 25/25 und 25/9, siehe CABLE.md | offen |
| 10 | Programmkopf (Zeichen vor dem Programm, z. B. `%`, Leader, Nullbytes) | keiner; Nullbytes werden entfernt | `transfer.program_start`, `strip_null_bytes` | HB2 11-30: Programm-Vorspann `%…` (Informationen), `?`, Prüfsumme; Dateien mit Kennung; Ende EOT | offen |
| 11 | Verwendung von M82 | nicht verwendet (Startsequenz leer) | `transfer.program_start` | – | offen |
| 12 | Programmende | `M30,M2` (laut PLC-Programm einer FP2A) | `transfer.program_end`, `receive.end_sequence` | – | teilweise (Quelle PLC) |
| 13 | Notwendige Zeichen nach M30 (CRLF? weitere?) | CRLF, danach `EOT` CRLF am Dateiende (in Parameterausgabe beobachtet); Nachlaufzeit 500 ms | `transfer.line_ending`, `transfer.send_eot_trailer`, `receive.end_on_eot`, `receive.end_grace_ms` | HB2 11-30: nach jeder Datei `?` und 4-stellige Prüfsumme, am Ende EOT | teilweise (EOT bei Parametern beobachtet, bei NC-Programmen offen) |
| 14 | Maximale Zeilenlänge | Warnung ab 120 Zeichen | `transfer.max_line_length` | – | offen |
| 15 | Maximale Programmgröße | Warnung ab 500 000 Bytes; Empfangslimit 2 000 000 Bytes | `transfer.max_program_bytes`, `receive.max_receive_bytes` | – | offen |
| 16 | Dateinamen-/Programmnummernformat | nicht ausgewertet → `RECEIVED_<Zeit>.nc` | `receive.program_name_pattern` | HB2 11-23/11-30: Programmnummer bis 8-stellig; Kennungen `%1234`, `%1234+%`, `%%567` usw. | offen |
| 17 | Verhalten bei Sonderzeichen (Tab, `;`, Kommentare in Klammern, Kleinbuchstaben) | Tabs → Leerzeichen; `;`-Zeilen und Kommentare werden beibehalten (Warnung) | `transfer.tab_handling`, `skip_semicolon_lines`, `comment_handling` | – | offen |
| 18 | Verhalten beim Empfang (Startbedienung an CNC, Pausen, Ende) | Ende per M30 + Zeilenende oder 5 s Inaktivität; Wartezeit auf erste Daten 120 s | `receive.*` | – | offen |
| 19 | Verhalten beim Senden (Einlesebereitschaft, notwendige Start-/Endverzögerung, Zeichenverzögerung) | 1000 ms Start-/Endverzögerung, keine Zeichen-/Paketverzögerung | `transfer.start_delay_ms`, `end_delay_ms`, `character_delay_ms`, `packet_*` | – | offen |
| 20 | Möglichkeit oder Unmöglichkeit von Drip-Feed (Nachladebetrieb) | über LSV2 implementiert, CNC-gesteuert über Quittungen; Nachladebetrieb laut Trainingshandbuch vorhanden (F3/F1/F8, Start in Betriebsart 2) | `protocol.*` | HB2 11-19 bis 11-22: Nachladen aus Datenspeicher-Gerät oder im DNC lokal; ≥ 25 KB frei, keine Schleifen | teilweise (Funktion dokumentiert, Protokoll offen) |
| 21 | Zeilenende, das die CNC beim Senden ausgibt | CRLF erwartet, andere werden normalisiert | `receive.normalize_line_endings` | – | offen |
| 22 | Kommentarzeichen der Steuerung | `(` … `)` | `transfer.comment_open/close` | HB2 11-32: Datei-Informationen stehen in runden Klammern | offen |
| 23 | Unterstützt die DIALOG 11 LSV2 / DIN 66019 überhaupt, und in welchen Betriebsarten? | ja (Annahme, Profil C) | `protocol.mode` | HB3: ja; für DNC-Betrieb Gerät 6 und „grundsätzlich“ LSV2 | offen |
| 24 | Schnittstellenparameter im LSV2-Betrieb | 4800 7E1, kein Handshake | `serial.*` (Profil C) | – | offen |
| 25 | Wer baut die Verbindung auf (ENQ vom PC oder Anforderung der CNC)? | PC sendet ENQ, alle 2 s, max. 120 s | `protocol.enq_interval_ms`, `link_timeout_ms` | HB3 Kap. 2: DNC lokal – Steuerung beginnt (`TDPP`/`RDPP`); DNC extern – Rechner beginnt | offen |
| 26 | Rahmung: transparent (`DLE STX/DLE ETX`) oder einfach (`STX/ETX`) | transparent | `protocol.transparent` | HB3 3-6a: `DLE STX` Daten `DLE ETX` BCC, DLE in den Daten verdoppelt | offen |
| 27 | Bildung des BCC | XOR über Text + ETX | `protocol.bcc_mode` | HB3: Rechenvorschrift nicht angegeben | offen |
| 28 | Quittungsfolge (DLE 0/DLE 1), Verhalten bei NAK, WACK, RVI | wie DIN 66019 Basisverfahren | – | HB3: `DLE 0` nach ENQ, `DLE 1` nach fehlerfreiem Block, sonst `NAK`; ENQ = Zustandsabfrage; je 3 Wiederholungen; kein WACK/RVI | offen |
| 29 | Inhalt eines Telegramms (eine Zeile? mit CRLF? maximale Länge?) | 1 Zeile inkl. CRLF, max. 256 Zeichen | `protocol.lines_per_telegram`, `include_line_ending`, `max_telegram_length` | HB3: höchstens 126 Zeichen je Block; Daten in DNC-Telegrammen `R_PP` | offen |
| 30 | Programmkopf / Start- und Endtelegramme im LSV2-Betrieb | keine | `protocol.start_telegrams`, `end_telegrams` | HB3 Kap. 2: `TDPP`/`PRPP`/`M_RR` … `T_FD` bzw. `T_BD` | offen |
| 31 | Maximale Zeit zwischen zwei Satzanforderungen im Nachladebetrieb | 10 min | `protocol.drip_feed_ack_timeout_ms` | HB3: Steuerung meldet `T_WA` (Speicher voll) und `T_NB` (bereit); Zeiten T_A 3 s, T_B 3 s, T_R 24 s, T_GZ 30 s | offen |
| 32 | Verhalten der CNC bei Abbruch (EOT vom PC, EOT von der CNC) und bei Unterbrechung der Datenzufuhr | PC sendet EOT bei Abbruch | `protocol.send_eot_on_cancel` | HB3: Abbruch über `T_BD` + Fehlernummer; nach 3 Fehlversuchen `EOT` | offen |
| 33 | Programmausgabe der CNC per LSV2 (Empfang) | ENQ von der CNC, Ende mit EOT | – | HB3: `RDPP` → `PTPP` → `M_RT` → `T_NB` → `R_PP`+Daten … `T_FD` | offen |
| 34 | Kann die DIALOG 11 Maschinenparameter über V.24 ausgeben bzw. einlesen? Bedienfolge an der Steuerung | angenommen: ja, vom Bediener ausgelöst | – | – | offen |
| 35 | Format der Parameterdatei (Kopf, Kennung, Zeilenaufbau, Ende) | `%MPGnn`/`%MDGnn (NAME)`, `Mnnnnn Wert`, `?` + 4-Hex-Prüfsumme, `EOT` – aus echter Ausgabe abgeleitet | `parameters.identification_pattern` | HB2 11-31: Kennung `%MDG11`; Ausgaben dieser Maschine: `%MPGnn (NAME)`, `Mnnnnn Wert`, `?`, 4-Hex-Prüfsumme | teilweise (Datei beobachtet, Einlesen offen) |
| 36 | Ende der Parameterausgabe | `EOT`, ersatzweise Inaktivität 5 s | `receive.end_on_eot`, `parameters.inactivity_timeout_ms` | drei echte Ausgaben: einmal Ende mit 0x04 (EOT), zweimal steht dort der Text `@004` (vermutlich vom Aufzeichnungsprogramm) | teilweise (EOT in Datei beobachtet) |
| 37 | Schutz der Parametereingabe (Schlüsselschalter, Schlüsselzahl, Betriebsart) | muss an der CNC freigegeben werden | – | – | offen |
| 38 | Akzeptiert die Steuerung eine vollständige Parameterdatei oder nur Teilmengen? | vollständige Datei | – | – | offen |
| 39 | Verhalten der Steuerung bei fehlerhafter oder abgebrochener Parametereingabe | unbekannt | – | – | offen |
| 40 | Gleiche Schnittstellenparameter/Protokoll wie für NC-Programme? | ja (gleiches Profil) | `serial.*`, `protocol.*` | – | offen |
| 44 | Format der Werkzeugdatei der DIALOG 11 (Ein-/Ausgabe über Daten-Transfer) | unbekannt – nur unverändertes Zurücksenden empfangener Dateien bzw. Vorlage (unverifiziert) | Vorlage in der Werkzeugverwaltung | HB2 11-33: Werkzeugtabelle `%…+T` mit Zeilen wie `T1 … R5 L120 FE… FZ200 S500` | offen |
| 42 | Prüfsummenverfahren der Parametergruppen | unbekannt – wird nie berechnet; geänderte Gruppe mit alter Prüfsumme wird nicht geschrieben | – | nachgerechnet: 16-Bit-Summe ab `%` bis vor `?`, ohne CR, LF, Leerzeichen – 69 von 69 Gruppen in drei Ausgaben stimmen | offen |
| 43 | Lehnt die Steuerung Parameterdateien eines anderen Softwarestands ab bzw. ist das gefährlich? | Schreiben nur bei gleichem Softwarestand und gleicher Maschinennummer wie die Sicherung | – | – | offen |
| 41 | Betrieb über RS-232/Ethernet-Adapter: Modell, Modus (TCP-Server/RFC 2217), Adapter-Einstellungen, Handshake im Adapter | nur gegen lokalen TCP-Testserver geprüft | Einstellungen → Verbindungsart | – | offen |

## Protokoll der Verifikation

| Datum | Maschine / Steuerungsstand | Profil | Geprüfte Punkte | Ergebnis | Prüfer |
|---|---|---|---|---|---|
| – | – | – | – | – | – |
