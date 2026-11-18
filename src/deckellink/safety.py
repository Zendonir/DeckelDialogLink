"""Safety texts and boundaries of the application."""

SAFETY_NOTICE = (
    "DeckelLink überträgt NC-Textdaten. Die Software prüft nicht, ob ein Programm geometrisch, "
    "technologisch oder maschinensicher ist. Prüfen Sie das Programm vollständig an der CNC und "
    "führen Sie den ersten Test ohne Werkstück beziehungsweise nach den betrieblichen "
    "Sicherheitsvorgaben durch."
)

SCOPE_NOTICE = (
    "DeckelLink überträgt ausschließlich Daten. Programmauswahl, Freigabe und Ausführung bleiben "
    "vollständig an der CNC-Steuerung. Es werden keine Zyklen gestartet, keine Achsen bewegt und "
    "keine Maschinenmodi umgeschaltet."
)

DRIP_FEED_NOTICE = (
    "NACHLADEBETRIEB: Die CNC arbeitet das Programm ab, während DeckelLink die Sätze nach "
    "Anforderung (Quittung) der Steuerung liefert. Den Programmstart nehmen Sie ausschließlich "
    "an der CNC vor. Ein Abbruch in DeckelLink, ein Kabel- oder PC-Fehler unterbricht nur die "
    "Datenzufuhr - die Maschine wird dadurch NICHT sicher angehalten. Halten Sie die Maschine bei "
    "Störungen an der Steuerung an. Der PC darf während des Nachladebetriebs nicht in den "
    "Energiesparmodus wechseln."
)

PARAMETER_WRITE_NOTICE = (
    "MASCHINENPARAMETER: Falsche Parameter können Kollisionen, Schäden an Maschine und Werkstück sowie "
    "Gefahren für Personen verursachen. DeckelLink prüft Parameter nicht inhaltlich - das Parameterformat "
    "der DIALOG 11 ist nicht verifiziert. Schreiben Sie nur Parameter, deren Bedeutung Sie kennen, nach den "
    "Vorgaben des Maschinenherstellers und mit Freigabe der verantwortlichen Person. Lesen Sie die Parameter "
    "nach dem Schreiben erneut aus und vergleichen Sie sie, bevor die Maschine verfährt."
)
