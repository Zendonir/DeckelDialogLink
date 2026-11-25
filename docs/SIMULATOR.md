# DIALOG-11-Simulator (Grafik, offline)

> **Nur Darstellung.** Der Simulator rechnet ein NC-Programm am PC durch und zeigt die Werkzeugbahnen. Er hat **keinen Zugriff** auf COM-Port, Netzwerkadapter oder Transport-Schicht und kann die Maschine weder steuern noch etwas übertragen. Das Ergebnis ersetzt keinen Testlauf an der Steuerung: Die Bahnen sind ein Hilfsmittel zum Lernen und zum Prüfen auf grobe Fehler.

## Öffnen

- **Werkzeuge → DIALOG-11-Simulator (Grafik, offline) …** (Strg+G).
- Ist auf der Seite *Übertragen* ein Programm geladen, öffnet der Simulator dieses Programm; sonst ein mitgeliefertes Demoprogramm.
- Mehrere Simulatorfenster können gleichzeitig offen sein.

Die Originaldatei wird nie verändert. Änderungen im Simulator lassen sich nur als **neue** Datei speichern; existiert der Name bereits, verweigert der Simulator das Speichern.

## Aufbau des Fensters

Das Fenster ist an den Aufbau der DIALOG 11 angelehnt (Hauptbetriebsarten, Betriebsartenanzeige, Softkeyleiste F1–F8). Es ist eine eigene Darstellung, keine Nachbildung des Originalbildschirms.

| Bereich | Inhalt |
|---|---|
| Gelbes Band | Kennzeichnung **SIMULATION – keine Verbindung zur Maschine** |
| Betriebsarten | **1 HANDFAHREN**, **2 AUTOMATIK**, **3 PROGRAMM**; daneben die Betriebsartenanzeige |
| links | Programmlisting; der aktuelle Satz ist gelb markiert |
| Mitte | Grafik: Draufsicht XY, Vorderansicht XZ, Seitenansicht YZ, 3D-Ansicht |
| rechts, *Anzeige* | Istwert (Werkstückkoordinaten), Restweg, F, S, Werkzeug, aktueller Satz, **Modalliste** |
| rechts, *Meldungen* | Fehler, Warnungen und Hinweise der Simulation (Doppelklick springt zur Zeile) |
| rechts, *Werkzeuge / Nullpunkte* | Werkzeugtabelle (Radius), Nullpunktspeicher G51.n, Parametertabelle, Eilgang-Annahme, Startposition |
| rechts, *INFO G/M* | Übersicht der ausgewerteten Befehle |
| unten | Zeitleiste, Laufzeit, Tempo (1× bis 1000×), Softkeys F1–F8 |

Grafik: Mausrad = Zoom, linke Maustaste = Verschieben, rechte Maustaste = Drehen (3D-Ansicht), Doppelklick = Einpassen. Farben: Vorschub hellblau, Eilgang orange gestrichelt, bereits bearbeitet grün, programmierte Kontur (ohne Bahnkorrektur) grau gepunktet, Werkzeug weiß (in der Draufsicht mit Radius).

### Softkeys

| Betriebsart | F1 | F2 | F3 | F4 | F5 | F6 | F7 | F8 |
|---|---|---|---|---|---|---|---|---|
| 1 HANDFAHREN | X − | X + | Y − | Y + | Z − | Z + | Schrittweite | INFO |
| 2 AUTOMATIK | Start (Folgesatz) | Einzelsatz | Halt | Anfang | Ende | Ansicht | Einpassen | INFO |
| 3 PROGRAMM | Grafik neu | Demo | Datei öffnen | Speichern unter (neue Datei) | Prüfen | Ansicht | Einpassen | INFO |

*Handfahren* bewegt nur das Werkzeug **im Simulator** (die Spur wird violett gezeichnet). In *Programm* wird nach jeder Änderung neu gerechnet. In *Automatik* setzt ein Klick in das Listing die Zeitleiste auf diesen Satz.

## Ausgewertete Programmiersprache

Grundlage sind die DECKEL-Trainingsunterlagen zur DIALOG 11 (Module 3–13). Die Unterlagen gehören nicht zum Repository. Die Tabelle *INFO G/M* im Simulator listet alle Befehle mit ihrem Umsetzungsstand. Im Überblick:

- **Sätze:** `N10 G1 X10 Y20; M3` – mehrere Teilsätze mit `;`, Kommentare in runden Klammern.
- **Koordinaten:** absolut/inkremental modal mit `G90`/`G91` oder wortweise `XA..`/`XI..`; polar mit `PX PY PW PL` oder `G9 I J W R`.
- **Wege:** `G0` (ohne Bahnabstimmung, Positionierlogik `G0.1`, Gerade `G0.2`), `G1`, `G2`/`G3` mit Mittelpunkt `I J K` (ohne Ziel = Vollkreis), Kreis mit `R` (Konturrechner), Schraubenlinien.
- **Ebenen und Korrektur:** `G17`–`G19`, Werkzeugaufruf `T`/`TT`, Bahnkorrektur `G41`/`G42`/`G40` mit An- und Wegfahren `G45`/`G46`/`G47 R..`.
- **Koordinatensysteme:** `G51.n`, `G53`, `G54`, `G55`/`G56` (Verschieben und Drehen mit `W`), `G58` Spiegeln, `G59` Maßstab.
- **Zyklen:** Bohren `G81`–`G86` (`OA/OI`, `TA/TI`, `SA/SI`, `EA/EI`, `MI`, `DI`, `ST`, `G4 F`), Rechtecktasche `G87.1`, Kreistasche `G87.2`, Zapfen `G87.3`, Gewindefräsen `G87.4`/`G87.5`, Konturtasche `G87.9 … M79`, Punktemuster `G89.1` (Gerade/Raster) und `G89.2` (Teilkreis).
- **Programmtechnik:** Unterprogramme `%*n`, Makros `%%n`, Makro-Unterprogramme `%%*n`, Aufruf n-mal mit `L.. %*n`, Programmteil-Wiederholung `L.. N.. N..`, Sprung `L0 N..`, `M17`, `M2`, `M30`.
- **Parameter:** `P..`/`PP..` (Speicher vor Tabelle), `PT..`/`PPT..` (nur Tabelle), Zuweisung `P1 = (P2 + 5)`, Wortketten `PP3 = G81 OA-10 TI15 SI2`, Ausführen `PP3`, Berechnen `P4!`, `IF P1 >= 3 THEN L0 N50 ELSE …` (auch mit `ELSE` in der Folgezeile), Rechenarten `+ - x /`, `SIN COS TAN ASIN ACOS ATAN SQR SQRT INT ABS RND FRC NEG PI`.

### Dateikonvention für Unterprogramme

Im Simulator stehen Haupt-, Unter- und Makroprogramme in **einem** Text: Eine Zeile, die nur `%*1`, `%%4` oder `%%*1` enthält (ohne Satznummer), beginnt ein neues Programm. Eine Zeile `%1` am Anfang benennt das Hauptprogramm. Diese Aufteilung ist eine Konvention des Simulators. Wie die DIALOG 11 zusammengehörige Dateien über V.24 ausgibt, ist nicht verifiziert.

## Vereinfachungen und Annahmen

Die Simulation meldet jede dieser Stellen als *Hinweis*, sobald sie im Programm vorkommt.

| Thema | Simulator | Grund |
|---|---|---|
| Schreibweise der G-Varianten (`G87 • 2`) | `G87.2`, `G87*2`, `G87+2` werden akzeptiert | Schreibweise in ASCII-Dateien nicht belegt |
| G18/G19 | G18: X/Z-Ebene, Zustellung Y; G19: Y/Z-Ebene, Zustellung X | Achsbelegung nicht dokumentiert |
| Eilgang | 5000 mm/min (einstellbar), `G0` mit gleicher Geschwindigkeit je Achse (Knickbahn) | Maschinenwert nicht hinterlegt |
| An-/Wegfahren `G45/G46/G47` | geometrisch vereinfacht | Details offen |
| `G60`/`G64`, `G61`/`G62`, Konturzug `UW UR FA` | nur Anzeige bzw. Hinweis, keine Berechnung | Konturrechner nicht nachgebildet |
| Kreis mit `R` | positives R = Bogen bis 180° | Konvention nicht belegt |
| Taschen/Zapfen | Ausräumbahnen als Ringe mit Abstand `AM` (ohne `AM`: 1,5 × Radius); Ende über Zyklusmitte auf Rückzugsebene | tatsächliche Strategie der Steuerung unbekannt |
| `G82` | kurzer Spanbrech-Rückzug nicht dargestellt | Weg nicht dokumentiert |
| `G83` | Wiederanfahren bis 1 mm über den Bohrgrund | Vorhalteabstand nicht dokumentiert |
| Konturtasche `G87.9` | Konturen auf Endtiefe, kein Ausräumen | Ausräumstrategie unbekannt |
| C-Achse, Schwenkachsen im Muster, Schmierhub | nur Anzeige bzw. Hinweis | – |
| M-Funktionen außer M2/M3/M4/M5/M6/M17/M30/M60–M62/M79 | ohne Wirkung | maschinenspezifisch |
| Werkzeuge, Nullpunkte, Parametertabelle | Tabellen im Simulatorfenster (nicht gespeichert) | Werkzeugspeicher der Steuerung nicht angebunden |

Grenzen gegen Endlosschleifen: höchstens 200 000 ausgeführte Sätze, 32 Aufrufebenen und 2 000 000 Bahnpunkte.
