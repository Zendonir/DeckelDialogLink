# Sichere Inbetriebnahme

Ziel: Die Kommunikation schrittweise und ohne Risiko für Maschine und Programme verifizieren. Arbeiten Sie nach den betrieblichen Sicherheitsvorgaben; die Maschine bleibt während der gesamten Inbetriebnahme unter Kontrolle des Bedieners.

## 1. Maschinenanschluss identifizieren
- Lage, Steckerform (DB9/DB25/andere) und Geschlecht der Buchse an der FP2A feststellen.
- Belegung aus den Maschinenunterlagen entnehmen. Keine Belegung „ausprobieren".

## 2. Einstellungen an der CNC dokumentieren
- Schnittstellenparameter der DIALOG 11 (Baudrate, Datenbits, Parität, Stoppbits, Handshake) aus den Maschinenparametern bzw. dem Einstellmenü ablesen und notieren.
- Keine Parameter an der Steuerung ändern, ohne die Folgen zu kennen.
- Werte in [VERIFICATION_CHECKLIST.md](VERIFICATION_CHECKLIST.md) eintragen.

## 3. Profil auswählen
- In DeckelLink unter *Einstellungen → Kommunikationsprofil* das passende Profil wählen bzw. eine Kopie mit den dokumentierten Werten anlegen.
- Profil bleibt `nicht verifiziert`, bis die Schritte 6–10 erfolgreich waren.

## 4. Kabel ohne Maschine prüfen
- Durchgang und Belegung mit Messgerät gegen die Dokumentation prüfen.
- Adapter im Gerätemanager prüfen (COM-Nummer, Treiber).

## 5. Loopback-Test durchführen
- Kabel von der Maschine trennen, am Maschinenende TX/RX brücken (bzw. am Adapter).
- *Diagnose → Loopback-Test*, Realbetrieb, richtiger COM-Port.
- Ergebnis „Bestanden" ist Voraussetzung für die nächsten Schritte. Bei Fehlern: [TROUBLESHOOTING.md](TROUBLESHOOTING.md).

## 6. Zuerst Programm von der CNC zum PC empfangen
- Empfangsrichtung zuerst, weil dabei nichts in die Steuerung geschrieben wird.
- *Empfangen → Empfang starten*, dann an der CNC die Ausgabe eines vorhandenen, bekannten Programms starten.

## 7. Rohbytes analysieren
- HEX-Vorschau und Terminal prüfen: Zeichen korrekt? Bytes > 0x7F (Paritätsproblem)? Welche Zeichen vor dem Programm und nach `M30`? Welches Zeilenende?
- Erkenntnisse in die Checkliste übernehmen und das Profil anpassen (Endsequenz, Zeilenende, Nullbytes).

## 8. Empfangene Datei sichern
- Datei unter *NC-Programme* prüfen und mit „Speichern unter" zusätzlich sichern. Die Metadaten (JSON) enthalten SHA-256 und Parameter.

## 9. Nur ein kleines, ungefährliches Testprogramm zurücksenden
- Ein kurzes Programm ohne Bewegungen bzw. mit unkritischem Inhalt erstellen (oder das gerade empfangene unverändert verwenden), unter **neuer** Programmnummer, falls die Steuerung das vorsieht.
- An der CNC das Einlesen starten, dann in DeckelLink senden. Den Bestätigungsdialog und den Sicherheitshinweis lesen.

## 10. Programm an der CNC prüfen
- Programm an der Steuerung vollständig anzeigen und mit dem Original vergleichen (Zeichen, Zeilen, Programmende).
- Optional: Programm erneut empfangen und SHA-256 der normalisierten Datei vergleichen.

## 11. Keine automatische Ausführung
- DeckelLink startet nie ein Programm. Die Ausführung erfolgt ausschließlich durch den Bediener an der CNC, beim ersten Lauf ohne Werkstück bzw. nach betrieblichen Vorgaben (Einzelsatz, reduzierter Vorschub, Probelauf).

## 12. Nachladebetrieb (LSV2) – erst nach den Schritten 1–11
1. Klären (Unterlagen), ob und wie die DIALOG 11 LSV2/Nachladebetrieb unterstützt; Profil C anpassen.
2. Zuerst **Speicherübertragung mit LSV2** (normales Senden) eines kleinen Programms testen; Terminal (HEX) auf `ENQ`, `DLE 0`, `DLE 1`, `NAK` prüfen.
3. Empfang per LSV2 testen (CNC gibt Programm aus).
4. Nachladebetrieb zunächst mit einem Programm **ohne Bearbeitung** (z. B. nur Verweilzeiten/Kommentare bzw. Probelauf ohne Werkstück) testen; Abbruch an der CNC und in DeckelLink ausprobieren und beobachten, wie die Steuerung reagiert.
5. Quittungs-Timeout an die längste Satzbearbeitungszeit anpassen.
6. Energiesparmodus des PCs und der USB-Anschlüsse deaktivieren.

Nach erfolgreichem Abschluss: Profil im Editor als „An realer Maschine verifiziert" markieren und die Checkliste mit Datum, Steuerungsstand und Ergebnis ergänzen.
