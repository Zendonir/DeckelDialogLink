# Programm- und Werkzeugverwaltung

Seite **9 Programmverwaltung** (auch über *Werkzeuge → Programm- und Werkzeugverwaltung*). Sie verwaltet Programme, Makros und Werkzeugdateien **auf dem PC**. An die Maschine geht eine Datei nur über die Seite *Übertragen* – mit der üblichen Prüfung und der Bestätigung unmittelbar vor dem Transfer.

## Programme

| Funktion | Wirkung |
|---|---|
| Importieren … | Kopiert Dateien in die Programmverwaltung (Originale bleiben unverändert, gleiche Namen erhalten `_001` usw.). Werkzeugaufrufe `T..`/`TT..` werden erkannt. |
| Angaben speichern | Name, Art (Hauptprogramm, Makro, Werkzeugdatei, Sonstige), Programmnummer an der Steuerung, Beschreibung |
| Neue Version aus Datei … | Ersetzt den Inhalt; der bisherige Stand wird vorher unter `versions/` gesichert. |
| Version wiederherstellen | Holt eine frühere Version zurück – ebenfalls als neue Version, nichts geht verloren. |
| Im Simulator bearbeiten | Öffnet den DIALOG-11-Simulator in *3 PROGRAMM*; „In Programmverwaltung speichern“ legt eine neue Version an. |
| Im Simulator ansehen | Grafik und Testlauf |
| Zum Senden laden | Lädt die Datei auf der Seite *Übertragen* (dort Prüfung, Senden mit Bestätigung). |
| Nachladebetrieb (LSV2) … | Lädt die Datei und startet den Nachladebetrieb – nur mit LSV2-Profil, mit eigener Bestätigung. Die Bearbeitung startet ausschließlich der Bediener an der CNC. |
| In Papierkorb | Verschiebt Datei, Angaben und Versionen nach `deleted/` – nichts wird endgültig gelöscht. |

Die **Werkzeugprüfung** zeigt, ob alle im Programm aufgerufenen Werkzeuge in der Werkzeugtabelle stehen. Nach einem erfolgreichen Senden wird „Zuletzt gesendet“ mit Zeit und Art (*Simulator* oder *real*) eingetragen.

Ablage: `<Datenordner>/library/` (`<Name>.nc` + `<Name>.json`, `versions/`, `deleted/`).

## Werkzeuge

Die Werkzeugtabelle (Nummer, Bezeichnung, Typ, Durchmesser, Länge, Schneiden, F, S, Bemerkung) liegt in `<Datenordner>/tools.json`; vor jedem Speichern wird eine Sicherung angelegt. Sie wird verwendet

- vom **Simulator** (Radius = Durchmesser/2 für Bahnkorrektur und Zyklen),
- für die **Werkzeugprüfung** der Programme.

## Werkzeugdaten an die Maschine

> **Das Format, in dem die DIALOG 11 Werkzeugdaten ein- und ausgibt, ist nicht bekannt.** DeckelLink erfindet keines.

Empfohlener Weg:

1. An der Steuerung die Werkzeugdatei ausgeben (Daten-Transfer), in DeckelLink auf der Seite *Empfangen* aufnehmen.
2. Die empfangene Datei in die Programmverwaltung importieren und die Art **Werkzeugdatei** wählen.
3. Diese Datei kann später **unverändert** zurückgesendet werden.

Optional kann unter *Werkzeuge* eine **Vorlage** (Kopfzeilen, eine Zeile je Werkzeug, Schlusszeilen; Platzhalter `{T} {NAME} {R} {D} {L} {Z} {F} {S} {TYP}`) hinterlegt werden – nur nach dem Muster einer echten, von der Steuerung ausgegebenen Datei. Daraus erzeugte Dateien sind als „Vorlage (nicht verifiziert)“ gekennzeichnet.

Beim Senden einer Werkzeugdatei zeigt der Bestätigungsdialog zusätzlich:

- dass Werkzeugdaten (Längen, Radien) an der Steuerung überschrieben werden und vor der Bearbeitung zu prüfen sind,
- bei Vorlagen-Dateien, dass das Format **nicht verifiziert** ist.
