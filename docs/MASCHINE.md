# DECKEL FP2A / DIALOG 11 – was die Maschine kann und was nicht

Stand: 28.11.2026, FP2A Nr. 2819-1110.

Quellen:
- **HB2:** Steuerungshandbuch 2, Kapitel 11 „Daten-Transfer" (Softwarestand 4)
- **HB3:** Steuerungshandbuch 3 „DNC-Protokoll und LSV2-Prozedur", 6003 04 024471 00, Stand 8.90 (Softwarestand 4)
- **Technische Daten:** Steuerungsdatenblatt im Handbuch-Scan
- **Maschinendaten:** drei Maschinendaten-Ausgaben dieser Maschine
- **EPROM-Analyse:** Firmware 4.274B und 7.07, Anwenderprogramme a4b092 und ctcaw19, SPS-Quelltext A0320

Was hier steht, ist aus diesen Unterlagen abgeleitet und nicht an der Maschine geprüft, sofern nicht ausdrücklich „beobachtet" dabeisteht. Für DeckelLink gelten weiterhin die offenen Punkte der [Verifikations-Checkliste](VERIFICATION_CHECKLIST.md).

## Steuerung und Softwarestand

| Karte | Aufgabe | Stand in der Maschine | Neuester gefundener Stand |
|---|---|---|---|
| NPP04 | CNC-Hauptrechner (68020) | 4.274 | 7.07 |
| NGE01 | Bildschirm, Grafik, Meldungstexte | 4.274, Texte „ENGL-D 05" | 7.06, Texte „CC 072" |
| NRP02.1 | Rechnerkarte | 4.274 | 7.06 |
| NSI02 | Schnittstellenkarte | 4.274 | 7.06 |
| NBP02 | Bedienpult | 4.274 | 7.06 |
| NPC01 | SPS-Prozessor | 7.07.01 (Anzeige sys 7.06) | 7.07.01 |
| NPM01 | SPS-Anwenderprogramm | a4b092 vom 23.09.1991 | – |
| NZP03 | Messkreiskarte für die Maßstäbe X, Y, Z (vierter Eingang für eine C-Achse) | – | – |

Die Maschine läuft mit NPC 7.07.01, a4b092 und allen übrigen Karten auf 4.274 (beobachtet). Ein Umstieg auf 7.0x geht nur mit allen CNC-Karten zugleich (NPP, NGE, NRP, NSI, NBP). Ein Versuch mit 7.07 auf NPP und NGE, aber 4.274 auf NSI, brachte beim Start eine Hardware-Meldung „NS…" (beobachtet).

Für DeckelLink heißt das: Alle Angaben der Handbücher beziehen sich auf Softwarestand 4. Ob sich die Schnittstelle unter 7.0x anders verhält, ist offen.

## Was die Maschine kann

| Funktion | Stand | Quelle |
|---|---|---|
| Programme, Unterprogramme, Makros, Werkzeug-, Parameter- und Nullpunkttabellen über die serielle Schnittstelle ein- und auslesen | vorhanden | HB2 11-8 bis 11-33 |
| Schnittstelle RS 232 oder TTY (20 mA) an einer 25-poligen Buchse, Kassettenrekorder an einer Klinkenbuchse | vorhanden | HB2 11-5, HB3 4-2 |
| Sieben speicherbare Schnittstellen-Konfigurationen (Gerät 1–7) | vorhanden | HB2 11-6 |
| DNC lokal (Bediener startet) und DNC extern (Rechner steuert) mit LSV2 | vorhanden | HB2 11-13, HB3 |
| Nachladen (Drip-Feed) aus einem Datenspeicher-Gerät oder im DNC lokal | vorhanden | HB2 11-19 bis 11-22 |
| Übertragung im Hintergrund, während ein Programm läuft | vorhanden | Trainingshandbuch, HB2 „Externer DNC-Betrieb" |
| Maschinendaten über V.24 ausgeben | beobachtet, Gruppen `%MPGnn` mit Prüfsumme | Maschinendaten |
| Schaltender Messtaster (Renishaw): Messfunktionen in der CNC, Logik in der SPS | vorbereitet, abgeschaltet (SPS-Parameter x53 = M33113 = 0) | EPROM-Analyse |
| Digitalisieren mit externem Digitalisier-PC | in der SPS vorhanden, abgeschaltet (x52 = M33112 = 0), Original-Hardware nötig | EPROM-Analyse |
| Freie Schaltfunktionen über M-Befehle | im Anwenderprogramm vorhanden (Modul SFKT1) | EPROM-Analyse |

## Was die Maschine nicht kann

| Funktion | Grund |
|---|---|
| Live-Status, Ist-Positionen oder Meldungen über V.24 abfragen | Das DNC-Protokoll kennt nur Programm- und Tabellentransfer, Inhaltsverzeichnis und Löschen (HB3 Kap. 2). |
| Achsen vom PC fahren oder die Bearbeitung vom PC starten | Nicht im DNC-Protokoll vorgesehen; für DeckelLink ohnehin ausgeschlossen ([SAFETY.md](SAFETY.md)). |
| Netzwerk (Ethernet) | Die Steuerung hat nur serielle Schnittstellen. Ein RS-232/Ethernet-Adapter ist möglich ([CABLE.md](CABLE.md)). |
| Spindel positionieren (M19) | In a4b092 nicht umgesetzt; M19 wird ohne automatischen Werkzeugwechsler (x54) sofort gelöscht. Ein Spindelgeber ist nicht bekannt. |
| Werkzeugmagazin, automatischer Werkzeugwechsel | Nicht vorhanden. Das Anwenderprogramm ctcaw19 gehört zu einer Maschine mit Magazin. |
| C-Achse | Nicht vorhanden. |

## SPS-Anwenderprogramm a4b092

Der Befehlssatz der SPS-Anwenderprogramme ist entschlüsselt; a4b092 ist zu 99,8 % als AWL lesbar. Das Programm ist aus rund 40 Modulen der A0320-Reihe aufgebaut. Hinter dem Code liegen 9.716 freie Worte, zwischen den Modulen 2.599 NOP-Worte. Eigene Funktionen könnten dort eingefügt werden. Offen ist, ob die Steuerung das Anwenderprogramm über eine Prüfsumme kontrolliert. Deshalb erst auf einem zweiten Chipsatz testen.

## Wege für eine PC-Kopplung

| Weg | Eingriff | Möglich |
|---|---|---|
| Programme senden, empfangen, nachladen über LSV2 | keiner | ja, Aufgabe von DeckelLink |
| Maschinenstatus über Optokoppler an den 24-V-Ausgängen mitlesen | keiner an der Steuerung | ja |
| Eigene Signale zwischen SPS und PC über freie Ein- und Ausgänge | Änderung im Anwenderprogramm | wahrscheinlich; Kandidaten sind I27.1–I27.7 und Q27.4–Q27.10, weil das Digitalisieren abgeschaltet ist |
| Neues Protokoll in der Steuerung | Umschreiben der 68000-Firmware | nicht realistisch |

**Achtung:** I27.0 (PCRFG) wirkt in a4b092 auch bei abgeschaltetem Digitalisieren auf die Reglerfreigabe. Diesen Eingang nicht belegen.

Details zur seriellen Übertragung: [DIALOG11.md](DIALOG11.md) und [CABLE.md](CABLE.md).
