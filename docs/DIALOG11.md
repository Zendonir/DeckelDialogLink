# DECKEL FP2A / DIALOG 11 – bekannte und angenommene Eigenschaften

> **Status:** Keine der folgenden Kommunikationseigenschaften wurde im Rahmen dieses Projekts an einer realen DIALOG 11 geprüft. Alle Tests liefen gegen den eingebauten Simulator. Seit dem 28.11.2026 liegen die Original-Steuerungshandbücher 2 und 3 (Softwarestand 4) vor; was daraus stammt, ist mit **HB2** bzw. **HB3** gekennzeichnet. Maßgeblich bleiben die Einstellungen an Ihrer Steuerung.
>
> Was die Maschine insgesamt kann und was nicht: [MASCHINE.md](MASCHINE.md).

## Schnittstelle

Die DIALOG 11 besitzt eine serielle V.24/RS-232-Schnittstelle zur Programmübertragung. Steckerform, Pinbelegung und konkrete Parameter hängen von Ausstattung, Softwarestand und Maschinenparametern ab und sind hier **nicht** festgelegt (siehe [CABLE.md](CABLE.md)).

### Angaben aus dem DECKEL-Trainingshandbuch (Modul 14 „Daten-Transfer", 1989)

Die folgenden Punkte stammen aus den Schulungsunterlagen. Sie beschreiben die Bedienung an der Steuerung, nicht die Einstellungen einer bestimmten Maschine:

- Die Schnittstelle lässt sich in der Unterbetriebsart **KONFIG I/O** für bis zu **7 Übertragungspartner** einstellen: Gerätenummer, Kanal, Bezeichnung, **Baudrate bis maximal 19200**, Datenbreite, Stoppbits, Parität, gegebenenfalls Übertragungsprotokoll und Partnerbezeichnung. Die Werte müssen im Partnergerät (also im DeckelLink-Profil) identisch eingestellt sein.
- Ein- und Auslesen erfolgt in der Hilfsbetriebsart **DATEN-TRANSFER** (Hauptbetriebsart 3). Im Eingabefeld werden das Gerät und Filter gewählt (welche zugehörigen Dateien nicht übertragen werden), in der BEFEHLE-Zeile die Programmnummer. Gestartet wird mit der **START**-Taste an der Steuerung.
- Übertragen werden Haupt- und Makroprogramme mit zugehörigen Dateien (Unterprogramme, Werkzeug- und Parameterdateien). Beim Einlesen wird das Programm unter der Nummer gespeichert, unter der es im Sendesystem archiviert war.
- **Parallelbetrieb:** Die Steuerung kann im Hintergrund (Hauptbetriebsart 3) ein- oder auslesen, während sie in Hauptbetriebsart 2 ein Programm abarbeitet.
- **Nachladebetrieb:** Nachladefunktion aktivieren (F3), EINLESEN wählen (F1) und starten (F8), dann in Hauptbetriebsart 2 das Programm anwählen (F5) und mit Zyklus-Start abarbeiten. Abgearbeitete Teile werden im Speicher gelöscht und „ratenweise" nachgeladen. Das deckt sich mit dem Grundsatz von DeckelLink: Der Start erfolgt immer an der CNC.
- **DNC-Betrieb** (z. B. mit einem Programmierplatz): „DNC lokal" (Initiative beim Bediener an der Steuerung) und „DNC extern" (Initiative beim Rechner, Zugriff auf den Programmspeicher). DeckelLink bietet **keinen** DNC-extern-Zugriff auf den Programmspeicher an (siehe [SAFETY.md](SAFETY.md)).

Das Trainingshandbuch nennt weder Protokoll noch Werte. Die Steuerungshandbücher 2 und 3 beschreiben beides (folgender Abschnitt). Welche Werte an Ihrer Steuerung eingestellt sind, bleibt in der [Verifikations-Checkliste](VERIFICATION_CHECKLIST.md) offen.

## Angaben aus den Steuerungshandbüchern (Softwarestand 4)

Quellen: **HB2** = Steuerungshandbuch 2, Kapitel 11 „Daten-Transfer"; **HB3** = Steuerungshandbuch 3 „DNC-Protokoll und LSV2-Prozedur", 6003 04 024471 00, Stand 8.90.

### Anschlüsse (HB2 11-5, HB3 4-2)

| Kanal | Anschluss | Verwendung |
|---|---|---|
| 1 | 25-polige Buchse an der Unterseite des Bedienpults | RS 232, TTY (20 mA), DNC |
| 2 | Option | wie Kanal 1 |
| 3 | Klinkenbuchse daneben | Kassettenrekorder, fest eingestellt |

Belegung und Originalkabel: [CABLE.md](CABLE.md).

### Schnittstellen-Konfiguration (HB2 11-6, 11-7)

In der Betriebsart 3 unter *Daten-Transfer → F6 Konfigurierung* sind sieben Geräte einstellbar:

- **Gerät 1–5:** frei für beliebige externe Geräte.
- **Gerät 6:** für den DNC-Betrieb. Laut HB3 sind dafür Gerät 6 und „grundsätzlich das Übertragungsprotokoll LSV2" zu wählen; übertragen wird über Kanal 1.
- **Gerät 7:** Kassettenrekorder.

| Einstellung | Mögliche Werte |
|---|---|
| Kanal | 1 oder 2 (25-polig), 3 (Kassette) |
| Art | RS 232 oder TTY |
| Baud | 75 bis 19200 |
| Datenbit | 7 oder 8 |
| Stopbit | 1 oder 2 |
| Parität | Even, None oder Odd |
| Protokoll | keins oder LSV2 |

Die technischen Daten der Steuerung nennen zusätzlich: „DNC/Nachladen: LSV 2 Protokoll, XON/XOFF auch ohne Protokoll bzw. RTS/CTS".

Werkseinstellungen nennt das Handbuch nicht. Ablesen lassen sie sich unter F6 „Konfigurierung" an der Steuerung.

### Betriebsarten der Übertragung (HB2 11-8 bis 11-22)

- **Einlesen/Auslesen** mit einem Datenspeicher-Gerät (Gerät 1–5): Bediener startet an der Steuerung (F1 Einlesen, F2 Auslesen, F8 Start).
- **DNC lokal** (F4, Gerät 6): Der Bediener startet Einlesen, Auslesen oder Nachladen; der Rechner antwortet nach dem DNC-Protokoll.
- **DNC extern** (F5, Gerät 6): Der Rechner sendet, holt, listet und löscht Programme. DeckelLink bietet das bewusst nicht an ([SAFETY.md](SAFETY.md)).
- **Nachladen** (F3) aus einem Datenspeicher-Gerät oder im DNC lokal. Bedingungen laut HB2 11-19:
  - mindestens 25 KB freier Speicher,
  - keine Programmschleifen, weil abgearbeitete Blöcke gelöscht werden,
  - Makros, Unterprogramme und Tabellen müssen vorher eingelesen sein.

  Im Programmverzeichnis erscheint der Status N.

## Dateiformat der Ausgabe (HB2 11-30 bis 11-33)

Die Steuerung gibt aus:

1. einen **Programm-Vorspann**: Programmkennung, Informationen, Ende-Kennung `?` und Prüfsumme,
2. die **Dateien** des Programms, jede mit Kennung, Informationen, Inhalt, Ende-Kennung `?` und Prüfsumme,
3. das Übertragungsende **EOT**.

Beispiel aus HB2 (Prüfsummen hier nur Platzhalter):

```
%8      (M-D11-0001E0- -14.11.88-Programmkopf)
?
xxxx
%8+%    (M-D11-000078- -14.11.88-Programmkopf)
N10 G0 Z100
...
N60 G0 Z100 M30
?
xxxx
<EOT>
```

| Datei | Kennung |
|---|---|
| NC-Programm (Vorspann) | `%1234` |
| Hauptprogramm | `%1234+%` |
| Unterprogramm | `%1234+56` |
| Lokale Werkzeuge | `%1234+T` |
| Lokale Parameter | `%1234+P` |
| Makro | `%%567` |
| Makro-Unterprogramm | `%%567+89` |
| Globale Werkzeuge | `%TT` |
| Globale Parameter | `%PP` ¹ |
| Nullpunkte | `%G51` |
| Maschinendaten | `%MDG11` |

¹ Im Handbuch steht bei `%PP` „Lokale Parameter“; nach der Ausgabereihenfolge ist die globale Parametertabelle gemeint (vermutlich Druckfehler).

Die Programmnummer ist bis zu achtstellig. Die **Informationen** stehen in runden Klammern, getrennt durch Bindestriche:

| Feld | Inhalt |
|---|---|
| Maßsystem | `M` metrisch, `I` Inch (entfällt bei T- und P-Tabellen) |
| Steuerung | `D11` = DIALOG 11, `D4` = DIALOG 4 (nur im Vorspann) |
| Länge | 6-stellig; die Beispiele (`0001E0`, `000078`) sind hexadezimal |
| Status | `R` Lesesperre, `W` Schreibschutz, `D` Löschschutz, `E` Bearbeitungssperre, `N` Nachladeprogramm |
| Datum | `TT.MM.JJ` |
| Kommentar | frei |

Fehlende Angaben werden als Leerzeichen geschrieben. Ausgelesen wird in fester Reihenfolge: lokale Werkzeuge, lokale Parameter, Hauptprogramme, Unterprogramme, Makros, Makro-Unterprogramme, globale Werkzeuge, globale Parameter, Nullpunkte, Maschinendaten.

Für **extern erstellte Programme** erlaubt HB2: Länge `000000` und Prüfsumme `0000`, wenn sie nicht berechnet wurden. Jede Datei braucht ihre Kennung; die Reihenfolge ist frei; am Ende steht EOT.

### Prüfsumme (nachgerechnet)

HB2 beschreibt die Prüfsumme als „Summe aller Zeichen" der Datei. An drei Maschinendaten-Ausgaben dieser FP2A (69 Gruppen `%MPGnn`) stimmt ausnahmslos folgende Regel:

> 16-Bit-Summe aller Zeichen vom `%` der Kennung bis vor die Zeile mit `?`, **ohne CR, LF und Leerzeichen**; ausgegeben als vier Hex-Ziffern in der Zeile nach `?`.

Für NC-Programme ist die Regel an echter Ausgabe noch zu prüfen. DeckelLink berechnet die Prüfsumme bisher nicht.

## Widersprüchliche Angaben zu Datenformat und Handshake

Öffentlich verfügbare Angaben (Foren, Erfahrungsberichte, Konfigurationen von DNC-Programmen) widersprechen sich. Häufig genannt werden u. a.:

| Variante | Baud | Daten | Parität | Stopp | Handshake | DeckelLink-Profil |
|---|---|---|---|---|---|---|
| A | 4800 | 7 | keine | 1 | RTS/CTS (Hardware) | `dialog11_7n1_rtscts` |
| B | 4800 | 7 | gerade (Even) | 1 | keiner | `dialog11_7e1_none` |

Laut HB2 sind Baudrate (75–19200), Datenbits (7/8), Parität (Even/None/Odd) und Stoppbits (1/2) je Gerät frei einstellbar; es gibt also keine feste „richtige“ Einstellung, sondern nur die an Ihrer Steuerung gewählte. DeckelLink behauptet **keine universell richtige Einstellung**. Beide Profile sind `unverified`. Sie sind Ausgangspunkte für die Inbetriebnahme; jede Abweichung (z. B. andere Baudrate, Odd-Parität, 2 Stoppbits, XON/XOFF) kann im Profil-Editor eingestellt werden.

Hinweis zur Fehlersuche: Eine falsche Parität zeigt sich typischerweise als vereinzelt oder durchgehend verfälschte Zeichen (DeckelLink meldet empfangene Bytes > 0x7F als möglichen Paritäts-/Datenbitfehler). Ein falscher Handshake zeigt sich als hängende Übertragung (CTS-Timeout) oder als Datenverlust bei längeren Programmen.

## Zeichensatz und Zeilenende

- Standard: 7-Bit-ASCII, Zeilenende CRLF (0x0D 0x0A).
- Umlaute und Steuerzeichen werden vor dem Senden als Fehler gemeldet.
- Ob die Steuerung Tabulatoren, Semikolonzeilen oder Kommentare in runden Klammern akzeptiert, ist **nicht verifiziert**; das Verhalten ist im Profil konfigurierbar.

## Programmende M30 / M2

`M30,M2` ist im Profil als **konfigurierbares** Programmende hinterlegt (mehrere Alternativen durch Komma getrennt; `M2` erkennt auch `M02`). Dass die FP2A ein Programm nach `M2` oder `M30` beendet, geht aus dem PLC-Programm einer realen Maschine hervor. Es dient
- der Prüfung („Programmende fehlt", „Daten nach Programmende"),
- der Enderkennung beim Empfang.

DeckelLink ergänzt ein Programmende niemals automatisch. Ein Ergänzen (immer die erste Alternative, also `M30`) ist nur möglich, wenn das Profil es erlaubt **und** der Benutzer es für die einzelne Übertragung ausdrücklich bestätigt. Welche Zeichen die Steuerung nach `M30` erwartet oder sendet, ist ein offener Prüfpunkt.

## Übertragungsende EOT

Dateien, die die DIALOG 11 ausgibt, enden mit dem Zeichen `EOT` (0x04) und CRLF (beobachtet an einer FP2A). DeckelLink
- akzeptiert `EOT` am Dateiende (Hinweis statt Fehler) und sendet es standardmäßig mit (`transfer.send_eot_trailer`); ein `EOT` mitten in der Datei bleibt ein Fehler,
- beendet den Empfang bei `EOT` (`receive.end_on_eot`) und übernimmt ein direkt auf `M30`/`M2` folgendes `EOT` in die Rohdaten.

## Startsequenz / M82

In einigen Quellen wird `M82` im Zusammenhang mit der Datenübertragung der DIALOG 11 erwähnt. Dessen Bedeutung und Notwendigkeit ist **unbestätigt**. Die Startsequenz im Profil ist daher standardmäßig **leer**; `M82` wird nur gesendet, wenn es ausdrücklich als Startsequenz eingetragen wird – dann erscheint eine Warnung.

## LSV2-Protokoll und NC-Nachladebetrieb

Für den NC-Nachladebetrieb (Abarbeiten, während die Sätze übertragen werden) wird das LSV2-Protokoll gefordert. DeckelLink implementiert dafür das **öffentliche DIN-66019- / ISO-1745-Basisverfahren**, auf dem LSV2 über V.24 aufsetzt:

| Element | Umsetzung in DeckelLink | Status |
|---|---|---|
| Verbindungsaufbau | PC sendet `ENQ` (wiederholt alle 2 s), CNC antwortet `DLE 0` | Annahme |
| Telegramm | `DLE STX` Text `DLE ETX` BCC, DLE im Text verdoppelt (alternativ `STX … ETX`) | Annahme, umschaltbar |
| Prüfzeichen | XOR über Text + ETX (alternativ über alle Zeichen nach STX, oder kein BCC) | Annahme, umschaltbar |
| Quittungen | abwechselnd `DLE 1` / `DLE 0`, `NAK` = Wiederholung, `DLE ;` (WACK) = warten | Annahme |
| Fehlende Quittung | PC fragt mit `ENQ` nach, CNC wiederholt letzte Quittung (keine Doppeltelegramme) | Annahme |
| Ende | `EOT` (beide Richtungen), `DLE <` (RVI) = Unterbrechung | Annahme |
| Telegramminhalt | standardmäßig eine NC-Zeile inkl. CRLF je Telegramm | Annahme, konfigurierbar |
| Start-/Endtelegramme | standardmäßig keine | konfigurierbar |

**Nicht** nachgebildet sind die anwendungsspezifischen LSV2-Kommandos (Dateioperationen, Login usw.), da deren Verwendung durch die DIALOG 11 nicht dokumentiert vorliegt. Falls die Steuerung einen Programmkopf-Telegramm erwartet, kann er als Starttelegramm (z. B. `%{name}`) eingetragen werden.

**Nachladebetrieb:** Die CNC bestimmt das Tempo – DeckelLink sendet das nächste Telegramm erst nach der Quittung der Steuerung. Das Quittungs-Timeout im Nachladebetrieb ist daher lang (Standard 10 min, 0 = unbegrenzt). DeckelLink startet die Bearbeitung nicht; der Programmstart erfolgt ausschließlich an der CNC. Siehe [SAFETY.md](SAFETY.md).

Mitgeliefertes Profil: `dialog11_lsv2_7e1` – *DIALOG 11 – LSV2 4800 7E1 (Nachladebetrieb)*, **nicht verifiziert**.

### LSV2 und DNC-Protokoll laut HB3

HB3 beschreibt zwei Schichten: die **LSV2-Prozedur** (Sicherung jedes einzelnen Telegramms, „in Anlehnung an DIN 66019, nicht genormt“) und darüber das **DNC-Protokoll** (Ablauf der Funktion). Jede Übertragung im DNC-Betrieb läuft über beide.

**LSV2-Prozedur (HB3 Kap. 3):**

| Element | Laut HB3 |
|---|---|
| Betrieb | asynchron, vollduplex, Punkt-zu-Punkt |
| Grundzustand | beide Stationen sind Datensenke und empfangsbereit |
| Ein Telegramm | Quelle `ENQ` → Senke `DLE 0` (empfangsbereit) → Quelle `DLE STX` Daten `DLE ETX` BCC → Senke `DLE 1` (fehlerfrei) oder `NAK` → Quelle `EOT` |
| Datenblock | höchstens 126 Zeichen; `DLE` in den Daten wird verdoppelt; längere Daten teilt die übergeordnete Schicht auf |
| Zustandsabfrage | Bleibt die Antwort aus, fragt die Quelle mit `ENQ`; die Senke wiederholt ihre letzte Antwort (`DLE 0`, `DLE 1` oder `NAK`) |
| Wiederholungen | je 3 für Empfangsaufforderung/Zustandsabfrage (N) und für die Datenphase (M); danach Abbruch mit `EOT` und Fehlermeldung |
| Kollision | Empfängt die Quelle selbst `ENQ`, hat die Gegenstation ebenfalls einen Sendewunsch (Zustand 11c) |
| Zeiten | T<sub>A</sub> Antwortzeit 3 s · T<sub>B</sub> Blockzeit 3 s · T<sub>R</sub> Reaktionszeit 24 s · T<sub>GZ</sub> Überwachung des Grundzustands 30 s (jeweils „Standardwert“) |
| Steuerzeichen | ENQ 0x05, DLE 0x10, STX 0x02, ETX 0x03, NAK 0x15, EOT 0x04 |
| BCC | „Blockcheck aus dem Datenblock berechnet“; die Rechenvorschrift nennt HB3 **nicht** |

`WACK` (`DLE ;`), `RVI` (`DLE <`) und wechselnde Quittungen über mehrere Blöcke kommen in HB3 nicht vor.

**DNC-Protokoll (HB3 Kap. 2):** Telegramme beginnen mit einer vierstelligen Kennung (`_` = Leerzeichen). Ein Telegramm mit unbekannter Funktion wird mit angehängtem `F` zurückgesendet.

| Ablauf | Telegrammfolge |
|---|---|
| Programm einlesen, DNC lokal | Steuerung `TDPP`+Kennung → PC `PRPP`+Kennung (nicht bereit: `TDPPF`) → Steuerung `M_RR` → PC `R_PP`+Daten → Steuerung `T_NB` … → PC `T_FD` (Ende) bzw. `T_BD` (Abbruch) |
| Programm auslesen, DNC lokal | Steuerung `RDPP`+Kennung → PC `PTPP`+Kennung → Steuerung `M_RT` → PC `T_NB` → Steuerung `R_PP`+Daten … `T_FD` |
| Nachladen, DNC lokal | wie Einlesen; die Steuerung meldet `T_WA`, wenn der Speicher voll ist, und `T_NB`, sobald sie wieder aufnehmen kann |
| DNC extern | PC beginnt mit `PRPP` (senden), `PTPP` (holen), `PTCD`/`PTPD` (Verzeichnis) oder `R_CL` (löschen) – in DeckelLink bewusst nicht umgesetzt |

Fehler und Abbrüche laufen über `T_BD` bzw. `xxxxF` mit angehängter Fehlernummer.

### Abweichungen der aktuellen DeckelLink-Umsetzung von HB3

Die LSV2-Umsetzung in DeckelLink entstand vor dem Vorliegen von HB3. Nach HB3 wird sie in dieser Form **voraussichtlich nicht** mit der DIALOG 11 zusammenarbeiten:

| Punkt | DeckelLink heute | Laut HB3 |
|---|---|---|
| Inhalt der Telegramme | NC-Zeilen direkt, Start-/Endtelegramme optional | Ablauf über DNC-Telegramme (`TDPP`/`PRPP`/`M_RR`/`R_PP`/`T_NB`/`T_FD` …), Daten nur in `R_PP`-Telegrammen |
| Initiative im DNC lokal | PC sendet `ENQ` von sich aus | Steuerung beginnt mit `TDPP` bzw. `RDPP`; der PC wartet als Datensenke |
| Quittungen | abwechselnd `DLE 1`/`DLE 0`, `WACK`, `RVI` | je Telegramm `DLE 0` nach `ENQ`, `DLE 1` nach fehlerfreiem Block, sonst `NAK` |
| Tempo im Nachladebetrieb | Steuerung hält die Quittung zurück (Timeout 10 min) | Steuerung quittiert jedes Telegramm und meldet `T_WA`/`T_NB` als eigene Telegramme |
| Blockgröße | bis 256 Zeichen | höchstens 126 Zeichen |
| Zeiten | `ENQ` alle 2 s, Quittung 5 s | T<sub>A</sub> 3 s, 3 Wiederholungen, T<sub>GZ</sub> 30 s |
| BCC | XOR über Text + ETX (umschaltbar) | Rechenvorschrift nicht angegeben, weiterhin offen |

Die Anpassung an HB3 ist noch nicht erfolgt; bis dahin bleibt das LSV2-Profil ausdrücklich `unverified`.

## Programmnamen

Laut HB2 hat jede Datei eine Kennung wie `%1234+%` und Informationen in runden Klammern (siehe „Dateiformat der Ausgabe“); an echter NC-Ausgabe dieser Maschine ist das Format noch nicht geprüft. Empfangene Programme heißen daher standardmäßig `RECEIVED_JJJJMMTT_HHMMSS.nc`. Ein regulärer Ausdruck (`receive.program_name_pattern`) kann nach Verifikation im Profil hinterlegt werden.

## Noch zu verifizieren

Siehe die vollständige Liste in [VERIFICATION_CHECKLIST.md](VERIFICATION_CHECKLIST.md).
