# Kabel und Schnittstelle

> **Wichtig:** Dieses Dokument enthält **keine** verbindliche Pinbelegung für die DIALOG 11. Eine unbestätigte Belegung kann Ein-/Ausgänge kurzschließen oder dazu führen, dass Handshake-Leitungen falsch bedient werden. Ermitteln Sie Steckerform und Belegung **aus den Maschinenunterlagen** (Schnittstellenbeschreibung, Schaltplan) oder durch eine fachkundige Messung.

## RS-232 vs. TTL-Seriell

| | RS-232 / V.24 | TTL-Seriell (UART) |
|---|---|---|
| Pegel | ca. ±3 … ±15 V (logisch 1 = negativ) | 0 / 3,3 V oder 0 / 5 V |
| Typischer Einsatz | Werkzeugmaschinen, Industrie-PCs | Mikrocontroller, Entwicklungsboards |

Die DIALOG 11 arbeitet nach V.24/RS-232. **USB-TTL-Adapter (z. B. „FTDI-Kabel" mit Stiftleiste) sind ungeeignet** und können beschädigt werden. Verwenden Sie einen echten USB-RS-232-Adapter mit Pegelwandler (z. B. FTDI- oder Prolific-Chipsatz mit RS-232-Treiber) oder eine native COM-Schnittstelle.

## DB9 und DB25

- **DB25** (25-polig) war bei älteren Steuerungen und Peripherie verbreitet.
- **DB9** (9-polig) ist am PC/Adapter üblich.
- Die Pinnummern der Signale **unterscheiden sich** zwischen DB9 und DB25. Ein mechanischer DB9-DB25-Adapter ist nur korrekt, wenn er für die gewünschte Rolle (DTE/DCE) beschaltet ist.

Gängige PC-seitige Belegung (DTE, DB9 nach EIA-574) – **nur PC-Seite, nicht Maschine**:

| Signal | DB9 (PC) | Richtung aus PC-Sicht |
|---|---|---|
| DCD | 1 | Eingang |
| RX (RxD) | 2 | Eingang |
| TX (TxD) | 3 | Ausgang |
| DTR | 4 | Ausgang |
| GND | 5 | – |
| DSR | 6 | Eingang |
| RTS | 7 | Ausgang |
| CTS | 8 | Eingang |
| RI | 9 | Eingang |

## Signale

- **TX / TxD** – Sendedaten.
- **RX / RxD** – Empfangsdaten.
- **GND** – Signalmasse (Bezugspotential; zwingend erforderlich).
- **RTS** – Request To Send: eigene Sende-/Empfangsbereitschaft (bei RTS/CTS-Handshake vom Treiber gesteuert).
- **CTS** – Clear To Send: Freigabe der Gegenstelle, Daten zu senden.
- **DTR** – Data Terminal Ready: eigene Betriebsbereitschaft.
- **DSR** – Data Set Ready: Betriebsbereitschaft der Gegenstelle.
- **DCD** – Data Carrier Detect: ursprünglich Modem-Träger; manchmal als Bereitschaftssignal verwendet.

## Nullmodem-Belegungen – Vorsicht

Im Internet kursieren viele „universelle" Nullmodem-Belegungen (vollständig gekreuzt, mit Loopback-Brücken RTS-CTS, DTR-DSR-DCD …). Eine solche Brücke **täuscht Handshake-Signale vor** und kann bewirken, dass der PC sendet, obwohl die Steuerung nicht empfangsbereit ist (Datenverlust). Verwenden Sie keine Belegung, die nicht zur Dokumentation Ihrer Maschine passt.

## Empfehlungen

- Geschirmtes Kabel, Schirm einseitig bzw. nach Vorgabe der Maschinendokumentation auflegen, möglichst kurze Leitung.
- Echter RS-232-Adapter mit stabilem Treiber; COM-Nummer im Gerätemanager notieren.
- Energiesparfunktion für USB-Anschlüsse am Werkstatt-PC deaktivieren.
- Kabel zuerst **ohne Maschine** prüfen (Durchgang, Loopback-Test in DeckelLink), siehe [COMMISSIONING.md](COMMISSIONING.md).
- Ermittelte Belegung in [VERIFICATION_CHECKLIST.md](VERIFICATION_CHECKLIST.md) dokumentieren.
