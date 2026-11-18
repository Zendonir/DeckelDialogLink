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

## Kein Drip-Feed

DeckelLink überträgt komplette Programme in den Speicher der Steuerung bzw. empfängt sie. Ein fest eingebautes Drip-Feed (Abarbeiten während der Übertragung) gibt es nicht, da nicht verifiziert ist, ob und wie die DIALOG 11 dies unterstützt, und weil es die Grenze zur Programmausführung berühren würde.

## Programmnamen

Das Format von Programmnummern/-namen im Datenstrom ist nicht verifiziert. Empfangene Programme heißen daher standardmäßig `RECEIVED_JJJJMMTT_HHMMSS.nc`. Ein regulärer Ausdruck (`receive.program_name_pattern`) kann nach Verifikation im Profil hinterlegt werden.

## Noch zu verifizieren

Siehe die vollständige Liste in [VERIFICATION_CHECKLIST.md](VERIFICATION_CHECKLIST.md).
