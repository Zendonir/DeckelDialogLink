//! Built-in demo program and the command overview shown in the simulator (INFO).

/// Own example program (not taken from the training documentation).
pub const DEMO_PROGRAM: &str = "(DECKELLINK SIMULATOR - DEMOPROGRAMM, NUR ZUR ANSICHT)\n\
N10 G0 Z100\n\
N20 G17 T1 M6; M3 S2000\n\
N30 X-60 Y-40 Z2\n\
N40 G1 Z-3 F150\n\
N50 G41 G47 R5 X-50 Y-30\n\
N60 G1 X40 F300\n\
N70 G3 X50 Y-20 I40 J-20\n\
N80 G1 Y20\n\
N90 G3 X40 Y30 I40 J20\n\
N100 G1 X-40\n\
N110 G1 X-50 Y20\n\
N120 G1 Y-30\n\
N130 G40 G47 R5\n\
N140 G0 Z50\n\
N150 G0 X0 Y0 Z2\n\
N160 G87.2 OA0 TI6 SI2 EA5 D30 AM4 MI3 G3\n\
N170 G0 X35 Y0\n\
N180 P1 = 0\n\
N190 G55 W60\n\
N200 G0 X35 Y0\n\
N210 G81 OA0 TI8 SI2 EA5\n\
N220 P1 = (P1 + 1)\n\
N230 IF P1 < 5 THEN L0 N190\n\
N240 G53\n\
N250 G0 X-35 Y-15\n\
N255 P2 = %*1\n\
N260 G89.1 LL4 LY10 P2\n\
N270 G0 Z100\n\
N280 M30\n\
%*1\n\
N10 G81 OA0 TI5 SI2 EA5\n\
";

/// (command, meaning, simulator support)
pub const FUNCTION_REFERENCE: &[(&str, &str, &str)] = &[
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
];
