# Kabel und Schnittstelle

> **Wichtig:** Die Belegung unten stammt aus dem Steuerungshandbuch 3 der DIALOG 11 (Softwarestand 4, Stand 8.90). An dieser FP2A ist sie noch nicht nachgemessen. Prüfen Sie Stecker und Belegung an Ihrer Maschine (Durchgangsprüfung, Schaltplan), bevor Sie ein Kabel anschließen; eine falsche Belegung kann Ein- und Ausgänge beschädigen.

## Belegung laut Steuerungshandbuch 3 (S. 4-2 bis 4-4)

### Anschluss an der Steuerung

25-polige Buchse an der Unterseite des Bedienpults („Universalschnittstelle“, Kanal 1). Die Steuerung ist **DTE** (Datenendeinrichtung).

| Pin | Signal | Gruppe |
|---|---|---|
| 1 | Schutzerde | – |
| 2 | TxD | V.24 |
| 3 | RxD | V.24 |
| 4 | RTS | V.24 |
| 5 | CTS | V.24 |
| 7 | GND | V.24 |
| 12 | +TTY 3 | TTY Senden |
| 24 | −TTY 4 | TTY Senden |
| 13 | +TTY 1 | TTY Empfangen |
| 25 | −TTY 2 | TTY Empfangen |
| 21 | GND | TTY |

DTR, DSR und DCD führt die Steuerung **nicht**. Der Kassettenrekorder (Kanal 3) hängt an einer Klinkenbuchse daneben (Kontakte Tx+, Tx−/Rx+, Rx−).

### Originalkabel 25-polig (Steuerung) auf 9-polig (PC)

| Steuerung (25-polig) | PC (9-polig) |
|---|---|
| 1 Abschirmung | Schirm nur auf Steuerungsseite |
| 2 TxD | 2 RxD |
| 3 RxD | 3 TxD |
| 4 RTS | 8 CTS |
| 5 CTS | 7 RTS |
| 7 GND | 5 GND |
| 20 DTR, mit 8 DCD gebrückt | 6 DSR |
| 6 DSR | 4 DTR, mit 1 DCD gebrückt |

### Originalkabel 25-polig auf 25-polig

| Steuerung | PC |
|---|---|
| 1 Abschirmung | Schirm nur auf Steuerungsseite |
| 2 TxD | 3 RxD |
| 3 RxD | 2 TxD |
| 4 RTS | 5 CTS |
| 5 CTS | 4 RTS |
| 7 GND | 7 GND |
| 20 DTR, mit 8 DCD gebrückt | 6 DSR |
| 6 DSR | 20 DTR, mit 8 DCD gebrückt |

Folgen für das Profil:
- RTS und CTS sind gekreuzt; ein RTS/CTS-Handshake ist damit elektrisch möglich.
- Weil die Steuerung an den Pins 6, 8 und 20 nichts anschließt, hängt **DSR am PC offen**. DSR darf im Profil nicht vorausgesetzt werden (`require_dsr_before_send` aus). DCD am PC folgt nur dem eigenen DTR.

## RS-232 vs. TTL-Seriell

| | RS-232 / V.24 | TTL-Seriell (UART) |
|---|---|---|
| Pegel | ca. ±3 … ±15 V (logisch 1 = negativ) | 0 / 3,3 V oder 0 / 5 V |
| Typischer Einsatz | Werkzeugmaschinen, Industrie-PCs | Mikrocontroller, Entwicklungsboards |

Die DIALOG 11 arbeitet nach V.24/RS-232; laut Handbuch kann derselbe Anschluss auch als TTY (20 mA) konfiguriert werden. **USB-TTL-Adapter (z. B. „FTDI-Kabel" mit Stiftleiste) sind ungeeignet** und können beschädigt werden. Verwenden Sie einen echten USB-RS-232-Adapter mit Pegelwandler (z. B. FTDI- oder Prolific-Chipsatz mit RS-232-Treiber) oder eine native COM-Schnittstelle.

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

Im Internet kursieren viele „universelle" Nullmodem-Belegungen (vollständig gekreuzt, mit Loopback-Brücken RTS-CTS, DTR-DSR-DCD …). Eine solche Brücke **täuscht Handshake-Signale vor** und kann bewirken, dass der PC sendet, obwohl die Steuerung nicht empfangsbereit ist (Datenverlust). Verwenden Sie keine Belegung, die nicht zur Dokumentation Ihrer Maschine passt. Das Originalkabel der DIALOG 11 (oben) hat solche Brücken nur auf DTR/DCD, nicht auf RTS/CTS.

## RS-232/Ethernet-Adapter (Device Server)

Statt eines USB-Adapters kann ein RS-232/Ethernet-Adapter (z. B. Moxa NPort, Lantronix, USR-TCP232) verwendet werden. DeckelLink unterstützt drei Varianten (*Einstellungen → Verbindung → Verbindungsart*):

| Verbindungsart | Adapter-Modus | Serielle Parameter | Modemleitungen in DeckelLink |
|---|---|---|---|
| COM-Port | Hersteller-Treiber mit virtuellem COM-Port („Real COM") | aus dem Profil | wie lokaler Port (abhängig vom Treiber) |
| TCP-Rohdaten | „TCP Server" | **im Adapter** einstellen (Baudrate, Datenformat, Handshake müssen zum Profil passen) | nicht sichtbar |
| RFC 2217 | „RFC 2217" / Telnet COM Port Control | aus dem Profil, von DeckelLink übertragen | CTS/DSR/DCD/RI werden gemeldet |

Hinweise:
- IP-Adresse und TCP-Port aus der Adapter-Konfiguration übernehmen (Moxa-Standard z. B. 4001 – im Adapter prüfen).
- Bei **TCP-Rohdaten** kann DeckelLink CTS/DSR nicht sehen. Profile mit „CTS vor dem Senden erforderlich" werden deshalb abgewiesen; ein Hardware-Handshake muss im Adapter konfiguriert werden. Für Nachladebetrieb mit LSV2 ist das unkritisch, da dort die Quittungen das Tempo bestimmen.
- Viele Adapter erlauben nur **eine** TCP-Verbindung gleichzeitig.
- Der Adapter sollte in einem abgeschotteten Maschinennetz stehen (kein Zugang aus dem Internet). DeckelLink selbst öffnet keine Ports und nutzt keine Cloud-Dienste.
- Die RS-232-Seite des Adapters ist wie ein PC-Anschluss zu behandeln (siehe oben); der Loopback-Test funktioniert auch über den Adapter.

## Empfehlungen

- Geschirmtes Kabel, Schirm einseitig bzw. nach Vorgabe der Maschinendokumentation auflegen, möglichst kurze Leitung.
- Echter RS-232-Adapter mit stabilem Treiber; COM-Nummer im Gerätemanager notieren.
- Energiesparfunktion für USB-Anschlüsse am Werkstatt-PC deaktivieren.
- Kabel zuerst **ohne Maschine** prüfen (Durchgang, Loopback-Test in DeckelLink), siehe [COMMISSIONING.md](COMMISSIONING.md).
- Ermittelte Belegung in [VERIFICATION_CHECKLIST.md](VERIFICATION_CHECKLIST.md) dokumentieren.
