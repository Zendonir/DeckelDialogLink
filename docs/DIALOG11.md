# DECKEL FP2A / DIALOG 11 – bekannte und angenommene Eigenschaften

> **Status:** Keine der folgenden Kommunikationseigenschaften wurde im Rahmen dieses Projekts an einer realen DIALOG 11 geprüft. Alle Tests liefen gegen den eingebauten Simulator. Maßgeblich sind die Maschinenunterlagen und die Einstellungen an Ihrer Steuerung.

## Schnittstelle

Die DIALOG 11 besitzt eine serielle V.24/RS-232-Schnittstelle zur Programmübertragung. Steckerform, Pinbelegung und konkrete Parameter hängen von Ausstattung, Softwarestand und Maschinenparametern ab und sind hier **nicht** festgelegt (siehe [CABLE.md](CABLE.md)).

## Widersprüchliche Angaben zu Datenformat und Handshake

Öffentlich verfügbare Angaben (Foren, Erfahrungsberichte, Konfigurationen von DNC-Programmen) widersprechen sich. Häufig genannt werden u. a.:

| Variante | Baud | Daten | Parität | Stopp | Handshake | DeckelLink-Profil |
|---|---|---|---|---|---|---|
| A | 4800 | 7 | keine | 1 | RTS/CTS (Hardware) | `dialog11_7n1_rtscts` |
| B | 4800 | 7 | gerade (Even) | 1 | keiner | `dialog11_7e1_none` |

DeckelLink behauptet **keine universell richtige Einstellung**. Beide Profile sind `unverified`. Sie sind Ausgangspunkte für die Inbetriebnahme; jede Abweichung (z. B. andere Baudrate, Odd-Parität, 2 Stoppbits, XON/XOFF) kann im Profil-Editor eingestellt werden.

Hinweis zur Fehlersuche: Eine falsche Parität zeigt sich typischerweise als vereinzelt oder durchgehend verfälschte Zeichen (DeckelLink meldet empfangene Bytes > 0x7F als möglichen Paritäts-/Datenbitfehler). Ein falscher Handshake zeigt sich als hängende Übertragung (CTS-Timeout) oder als Datenverlust bei längeren Programmen.

## Zeichensatz und Zeilenende

- Standard: 7-Bit-ASCII, Zeilenende CRLF (0x0D 0x0A).
- Umlaute und Steuerzeichen werden vor dem Senden als Fehler gemeldet.
- Ob die Steuerung Tabulatoren, Semikolonzeilen oder Kommentare in runden Klammern akzeptiert, ist **nicht verifiziert**; das Verhalten ist im Profil konfigurierbar.

## Programmende M30

`M30` ist im Profil als **konfigurierbares** Programmende hinterlegt. Es dient
- der Prüfung („Programmende fehlt", „Daten nach Programmende"),
- der Enderkennung beim Empfang.

DeckelLink ergänzt `M30` niemals automatisch. Ein Ergänzen ist nur möglich, wenn das Profil es erlaubt **und** der Benutzer es für die einzelne Übertragung ausdrücklich bestätigt. Welche Zeichen die Steuerung nach `M30` erwartet oder sendet, ist ein offener Prüfpunkt.

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

## Programmnamen

Das Format von Programmnummern/-namen im Datenstrom ist nicht verifiziert. Empfangene Programme heißen daher standardmäßig `RECEIVED_JJJJMMTT_HHMMSS.nc`. Ein regulärer Ausdruck (`receive.program_name_pattern`) kann nach Verifikation im Profil hinterlegt werden.

## Noch zu verifizieren

Siehe die vollständige Liste in [VERIFICATION_CHECKLIST.md](VERIFICATION_CHECKLIST.md).
