# Sicherheitskonzept

DeckelLink ist ein Werkzeug für eine industrielle Maschinenumgebung. Es überträgt **ausschließlich NC-Textdaten**.

## Was DeckelLink nie tut
- keinen CNC-Zyklus und keine Programmausführung starten oder freigeben,
- keine Achsen bewegen, keine Maschinenmodi umschalten,
- Maschinenparameter aus eigenem Antrieb ändern oder Fernbefehle zum Lesen/Ändern von Parametern senden (Parameter werden nur als Datei in eine an der CNC freigegebene Parametereingabe übertragen),
- keine sicherheitsrelevanten Maschinenfunktionen ansteuern,
- keine Ausführungsbefehle an die CNC senden (das Terminal ist reine Anzeige),
- nicht automatisch senden – weder beim Start noch nach Dateiauswahl,
- im Nachladebetrieb die Bearbeitung starten (sie liefert nur Sätze, die die CNC per Quittung anfordert),
- Originaldateien verändern oder Dateien stillschweigend überschreiben,
- behaupten, ein Programm sei fertigungssicher.

### Warum keine Fernsteuerung?
Programmstart, Achsbewegungen, Betriebsartenwahl oder ein frei beschreibbares Befehls-Terminal sind bewusst nicht enthalten:
- Für die DIALOG 11 ist keine Fernsteuerschnittstelle über V.24 dokumentiert. Befehle müssten erfunden werden – unbekannte Zeichenfolgen an eine Werkzeugmaschine zu senden, ist nicht beherrschbar.
- Bewegungen und Programmstarts setzen Anwesenheit und Sicht des Bedieners, Zustimmtaster und erreichbaren NOT-HALT an der Maschine voraus. Eine Fernsteuerung über eine serielle oder Netzwerkverbindung würde dieses Sicherheitskonzept umgehen und wäre eine wesentliche Veränderung der Maschine mit neuer Risikobeurteilung (Maschinenrichtlinie 2006/42/EG bzw. Maschinenverordnung (EU) 2023/1230).

## Schutzmaßnahmen in der Software
| Maßnahme | Umsetzung |
|---|---|
| Simulationsmodus als Standard | Erster Start im Simulator; gelb-schwarzes Banner; Realbetrieb rot gekennzeichnet |
| Bestätigung unmittelbar vor jedem Transfer | Dialog mit Datei, Profil, Port, Datenformat; Standardknopf „Abbrechen" |
| Sicherheitshinweis beim realen Transfer | Abschaltbar, in den Einstellungen wieder aktivierbar |
| Prüfung vor dem Senden | Fehler sperren; Warnungen müssen bestätigt werden |
| Original bleibt unverändert | Transformation erzeugt separate Daten |
| Kein stilles Überschreiben | exklusives Anlegen, `_001`-Namensschema, Backup vor Ersetzen |
| Abbruch jederzeit | Abbrechen-Knopf/Esc, kooperativ, Port wird kontrolliert geschlossen |
| Unvollständige Empfänge | Ablage in `failed/`, Kennzeichnung „UNVOLLSTÄNDIG" |
| M30/M82 | nie automatisch; nur per Profil und ausdrücklicher Bestätigung |
| Datenschutz in Logs | keine NC-Inhalte im Standardlog; Rohdatenlog nur nach Aktivierung |

## Nachladebetrieb (LSV2)

Im Nachladebetrieb arbeitet die Maschine, während Daten übertragen werden. Zusätzliche Maßnahmen:

| Maßnahme | Umsetzung |
|---|---|
| Nur mit LSV2-Profil | Schaltfläche „Nachladebetrieb (LSV2)" erscheint nur bei aktivem LSV2-Profil |
| Eigener Warnhinweis + Pflichtbestätigung | Starten erst nach Ankreuzen „Maschine muss bei Störungen an der CNC angehalten werden" (auch im Simulator) |
| CNC bestimmt das Tempo | nächstes Telegramm erst nach Quittung der Steuerung |
| Kein Standby | Windows-Energiesparmodus wird während des Nachladebetriebs unterdrückt |
| Abbruch | sendet EOT (konfigurierbar); **hält die Maschine nicht an** |

> Ein Abbruch in DeckelLink, ein Kabel- oder PC-Fehler unterbricht nur die Datenzufuhr. Die Maschine wird dadurch nicht sicher angehalten – das muss an der Steuerung geschehen.

## Maschinenparameter

Schreiben ist mehrfach abgesichert (Details: [PARAMETERS.md](PARAMETERS.md)): Profilfreigabe (Standard aus), Freischaltung nur für die Sitzung, aktuelle Sicherung (≤ 60 min, gleiches Profil/Anschluss), Prüfung und Diff, getippte Bestätigung `PARAMETER SCHREIBEN`, anschließende Kontrolle durch erneutes Auslesen. Die Parametereingabe muss an der CNC durch eine berechtigte Person freigegeben werden – DeckelLink kann sie nicht freischalten.

## Sicherheitshinweis (Wortlaut)

> DeckelLink überträgt NC-Textdaten. Die Software prüft nicht, ob ein Programm geometrisch, technologisch oder maschinensicher ist. Prüfen Sie das Programm vollständig an der CNC und führen Sie den ersten Test ohne Werkstück beziehungsweise nach den betrieblichen Sicherheitsvorgaben durch.

## Verantwortung des Betreibers
Die Prüfung und Freigabe jedes Programms an der Maschine, die Einhaltung der Betriebsanweisungen und die Inbetriebnahme der Schnittstelle (siehe [COMMISSIONING.md](COMMISSIONING.md)) liegen beim Betreiber. Die Software wird ohne Gewährleistung bereitgestellt (MIT-Lizenz).
