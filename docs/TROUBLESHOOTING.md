# Fehlersuche

Zu jedem Fehler zeigt DeckelLink eine verständliche Meldung mit möglichen Ursachen; die technischen Details sind einklappbar und stehen im Log (*Protokoll → Log-Ordner öffnen*).

## Port lässt sich nicht öffnen
| Meldung | Ursachen / Abhilfe |
|---|---|
| „COMx konnte nicht geöffnet werden – der Anschluss ist belegt." | Anderes Programm (Terminal, DNC-Software, zweite DeckelLink-Instanz) schließen. Adapter ab- und anstecken. |
| „COMx wurde nicht gefunden." | Adapter getrennt oder neue COM-Nummer nach Umstecken → *Einstellungen → Ports suchen*. Treiber im Gerätemanager prüfen. |
| „COMx konnte nicht geöffnet werden." | Treiber fehlt/defekt; Rechte; Port existiert nicht mehr. |

## Senden hängt bzw. „Sendefreigabe (CTS) nicht erteilt"
- An der CNC wurde das Einlesen nicht gestartet.
- Profil mit RTS/CTS, aber Kabel führt CTS nicht → Kabel prüfen oder (nach Verifikation!) Profil ohne Handshake.
- Adapter unterstützt Modemleitungen nicht → *Diagnose → Port zur Diagnose öffnen* und CTS/DSR beobachten.

## Zeitüberschreitung während des Sendens
- Steuerung hält per Handshake an (Speicher voll, Einlesen beendet).
- Falsche Baudrate/Parität: Steuerung verwirft Daten.
- Zeichen-/Paketverzögerung im Profil versuchen, falls die Steuerung langsam einliest (nur nach Rücksprache mit Unterlagen).

## Empfangene Zeichen sind verfälscht
- Bytes > 0x7F oder vereinzelt falsche Zeichen → **Parität** oder Datenbits stimmen nicht (7N1 vs. 7E1).
- Durchgehend unlesbar → **Baudrate** falsch.
- Störungen: geschirmtes, kürzeres Kabel, Masseführung prüfen.

## Empfang endet nicht oder zu früh
- Endet nicht: Endsequenz (Standard `M30`) wird nicht gesendet → Inaktivitätszeit beachten (Standard 5 s) bzw. Empfangsende auf „Endsequenz oder Inaktivität" stellen.
- Endet zu früh: Steuerung macht Pausen > Inaktivitätszeit → Inaktivitäts-Timeout erhöhen.
- „Maximale Empfangsgröße überschritten" → Limit im Profil erhöhen (0 = unbegrenzt, nicht empfohlen).

## Loopback-Test schlägt fehl
- TX und RX nicht gebrückt; falscher COM-Port; Port belegt.
- Bei RTS/CTS-Profil zusätzlich RTS–CTS brücken.
- TTL-Adapter statt RS-232 (kein echter Pegel).
- „Teilweise bestanden": Wackelkontakt, Störung, ungeeigneter Adapter.

## Prüfung meldet Fehler
- „Zeichen … nicht darstellbar": Umlaute/Sonderzeichen entfernen (Datei im Editor ändern – DeckelLink verändert keine Originale).
- „Unzulässige Steuerzeichen": Datei aus einem Editor speichern, der reinen Text erzeugt.
- „Tabulatoren … abgelehnt": Profiloption *Tabulatoren* auf „durch Leerzeichen ersetzen".

## Profil ungültig
Die fehlerhaften Felder werden unter *Einstellungen* angezeigt; DeckelLink verwendet dann ein sicheres Standardprofil. Profildateien liegen im Ordner `profiles/` des Datenverzeichnisses.

## Anwendung startet nicht (portable Version)
- ZIP vollständig entpacken (nicht aus dem ZIP heraus starten).
- `DeckelLink.exe --smoke-test --smoke-report bericht.json` ausführen und Bericht prüfen.
- Logs: `%LOCALAPPDATA%\DeckelLink\Logs\deckellink.log`.

## Diagnosepaket
*Diagnose → Diagnosepaket exportieren*: Logs, anonymisierte Einstellungen, Profil, Systeminfo, letztes Ergebnis. Pfade werden standardmäßig maskiert, NC-Dateien und Rohdaten nur auf ausdrücklichen Wunsch.
