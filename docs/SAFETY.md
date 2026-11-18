# Sicherheitskonzept

DeckelLink ist ein Werkzeug für eine industrielle Maschinenumgebung. Es überträgt **ausschließlich NC-Textdaten**.

## Was DeckelLink nie tut
- keinen CNC-Zyklus und keine Programmausführung starten oder freigeben,
- keine Achsen bewegen, keine Maschinenmodi umschalten, keine Maschinenparameter ändern,
- keine sicherheitsrelevanten Maschinenfunktionen ansteuern,
- keine Ausführungsbefehle an die CNC senden (das Terminal ist reine Anzeige),
- nicht automatisch senden – weder beim Start noch nach Dateiauswahl,
- Originaldateien verändern oder Dateien stillschweigend überschreiben,
- behaupten, ein Programm sei fertigungssicher.

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

## Sicherheitshinweis (Wortlaut)

> DeckelLink überträgt NC-Textdaten. Die Software prüft nicht, ob ein Programm geometrisch, technologisch oder maschinensicher ist. Prüfen Sie das Programm vollständig an der CNC und führen Sie den ersten Test ohne Werkstück beziehungsweise nach den betrieblichen Sicherheitsvorgaben durch.

## Verantwortung des Betreibers
Die Prüfung und Freigabe jedes Programms an der Maschine, die Einhaltung der Betriebsanweisungen und die Inbetriebnahme der Schnittstelle (siehe [COMMISSIONING.md](COMMISSIONING.md)) liegen beim Betreiber. Die Software wird ohne Gewährleistung bereitgestellt (MIT-Lizenz).
