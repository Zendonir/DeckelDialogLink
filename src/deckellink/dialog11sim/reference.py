"""Built-in demo program and the command overview shown in the simulator (INFO)."""

from __future__ import annotations

# Own example program (not taken from the training documentation).
DEMO_PROGRAM = """(DECKELLINK SIMULATOR - DEMOPROGRAMM, NUR ZUR ANSICHT)
N10 G0 Z100
N20 G17 T1 M6; M3 S2000
N30 X-60 Y-40 Z2
N40 G1 Z-3 F150
N50 G41 G47 R5 X-50 Y-30
N60 G1 X40 F300
N70 G3 X50 Y-20 I40 J-20
N80 G1 Y20
N90 G3 X40 Y30 I40 J20
N100 G1 X-40
N110 G1 X-50 Y20
N120 G1 Y-30
N130 G40 G47 R5
N140 G0 Z50
N150 G0 X0 Y0 Z2
N160 G87.2 OA0 TI6 SI2 EA5 D30 AM4 MI3 G3
N170 G0 X35 Y0
N180 P1 = 0
N190 G55 W60
N200 G0 X35 Y0
N210 G81 OA0 TI8 SI2 EA5
N220 P1 = (P1 + 1)
N230 IF P1 < 5 THEN L0 N190
N240 G53
N250 G0 X-35 Y-15
N255 P2 = %*1
N260 G89.1 LL4 LY10 P2
N270 G0 Z100
N280 M30
%*1
N10 G81 OA0 TI5 SI2 EA5
"""

DEMO_PARAMETERS: dict[str, str] = {}

# (command, meaning, simulator support)
FUNCTION_REFERENCE: tuple[tuple[str, str, str], ...] = (
    ("G0", "Eilgang, Achsen ohne Bahnabstimmung", "ja (Achsen einzeln, Knickbahn)"),
    ("G0.1", "Eilgang mit Positionierlogik (Zustellachse zuerst/zuletzt je nach Richtung)", "ja"),
    ("G0.2", "Eilgang mit Geradeninterpolation", "ja"),
    ("G1", "Gerade im Vorschub", "ja"),
    ("G2 / G3", "Kreis im / gegen den Uhrzeigersinn, Mittelpunkt I J K, ohne Ziel = Vollkreis", "ja"),
    ("G9", "Zielpunkt polar: Pol I J K, Winkel W (WA/WI), Länge R", "ja"),
    ("PX PY PW PL", "Zielpunkt polar mit Konturrechner", "ja (ohne Konturrechner-Lösung)"),
    ("G4 F..", "Verweilzeit in Sekunden", "ja (in der Laufzeit)"),
    ("G17 / G18 / G19", "Arbeitsebene und Zustellrichtung", "ja (G18/G19 Achsbelegung angenommen)"),
    ("G40 / G41 / G42", "Bahnkorrektur aus / links / rechts der Kontur", "ja"),
    ("G45 / G46 / G47 R..", "An- und Wegfahren tangential / Halbkreis / Viertelkreis", "vereinfacht"),
    ("G51.n", "Nullpunkt aus dem Nullpunktspeicher", "ja (Simulator-Tabelle)"),
    ("G53", "Nullpunktverschiebungen und Drehung aufheben", "ja"),
    ("G54 X..", "Istposition neue Koordinaten zuweisen", "ja"),
    ("G55 X.. W..", "Nullpunkt additiv verschieben / drehen", "ja"),
    ("G56 X.. W..", "Nullpunkt absolut verschieben / drehen", "ja"),
    ("G58 X..", "Spiegeln an Achse (ohne Angabe: aus)", "ja"),
    ("G59 X..", "Maßstab je Achse (ohne Angabe: aus)", "ja"),
    ("G60 / G64", "Fahrverhalten an Ecken", "nur Anzeige"),
    ("G61 / G62", "Radius-/Fasenübergang an Ecken", "nur Hinweis"),
    ("G81 - G86", "Bohren, Spanbrechen, Tieflochbohren, Gewindebohren, Reiben, Ausdrehen", "ja"),
    ("G87.1 / .2 / .3", "Rechtecktasche, Kreistasche, Zapfen", "ja (Ausräumbahnen vereinfacht)"),
    ("G87.4 / .5", "Gewinde fräsen außen / innen", "vereinfacht"),
    ("G87.9 ... M79", "Konturtasche", "nur Konturen auf Endtiefe"),
    ("G89.1 / G89.2", "Punktemuster auf Gerade / Teilkreis", "ja"),
    ("G90 / G91", "Absolut- / Kettenmaß; wortweise XA.. / XI..", "ja"),
    ("UW UR FA", "Konturzug-Übergänge", "nein (Hinweis)"),
    ("T.. / TT..", "Werkzeugaufruf (programmbezogen / allgemein)", "ja (Simulator-Werkzeugtabelle)"),
    ("F.. / S..", "Vorschub mm/min / Drehzahl", "ja"),
    ("M2 / M30", "Programmende / mit Rücksprung", "ja"),
    ("M3 / M4 / M5", "Spindel rechts / links / halt", "nur Anzeige"),
    ("M6", "Werkzeugwechsel", "nur Anzeige"),
    ("M17", "Ende Unterprogramm", "ja"),
    ("M60 / M61 / M62", "Vorschubverhalten an der Kontur", "nur Anzeige"),
    ("L.. N.. N..", "Programmteil-Wiederholung", "ja"),
    ("L0 N..", "Sprung", "ja"),
    ("%*n / %%n / L.. %*n", "Unterprogramm / Makro aufrufen (n-mal)", "ja (im selben Text)"),
    ("P.. = .. / PP..", "Parameter zuweisen / globaler Parameter", "ja"),
    ("P..!", "Parameter laut Tabelle berechnen", "ja"),
    ("IF .. THEN .. ELSE ..", "Parametervergleich mit Sprung oder Zuweisung", "ja"),
    ("C..", "Rundtisch (C-Achse)", "nur Anzeige"),
)
