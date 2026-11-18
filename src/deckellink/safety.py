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
