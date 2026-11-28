"""Menu structure of the DIALOG 11 as reproduced in the simulator.

Main modes, sub-modes and auxiliary modes with their softkey numbers follow
the structure overview of the training documentation (module 4). Codes marked
``assumed`` are not legible there and were inferred from their neighbours.
Descriptions are DeckelLink's own short texts. ``available=False`` marks
machine functions the simulator does not reproduce (they only show an
explanation).
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class MenuItem:
    code: str
    label: str
    description: str
    action: str = ""
    children: tuple[MenuItem, ...] = ()
    available: bool = True
    assumed: bool = False
    key: int = 0  # softkey position 1..8 (0 = derived from the last digit of the code)

    @property
    def softkey(self) -> int:
        if self.key:
            return self.key
        digit = self.code[-1:] if self.code else ""
        return int(digit) if digit.isdigit() and digit != "0" else 0

    @property
    def is_menu(self) -> bool:
        return bool(self.children)

    @property
    def title(self) -> str:
        label = self.label.replace("-\n", "-").replace("\n", " ")
        return f"{self.code} {label}".strip() if self.code and self.code != self.label else label

    def slots(self) -> list[MenuItem | None]:
        """Children placed on softkeys F1..F8."""
        slots: list[MenuItem | None] = [None] * 8
        for child in self.children:
            position = child.softkey
            if 1 <= position <= 8 and slots[position - 1] is None:
                slots[position - 1] = child
        return slots

    def find(self, code: str) -> MenuItem | None:
        if self.code == code:
            return self
        for child in self.children:
            found = child.find(code)
            if found is not None:
                return found
        return None

    def walk(self, depth: int = 0) -> list[tuple[int, MenuItem]]:
        out = [(depth, self)]
        for child in self.children:
            out.extend(child.walk(depth + 1))
        return out


def _m(
    code: str,
    label: str,
    description: str,
    *children: MenuItem,
    action: str = "",
    available: bool = True,
    assumed: bool = False,
    key: int = 0,
) -> MenuItem:
    return MenuItem(code, label, description, action, tuple(children), available, assumed, key)


BACK = "back"
NOT_SIMULATED = "Maschinenfunktion - im Simulator nicht nachgebildet."

MAIN_1 = _m(
    "1",
    "HANDFAHREN",
    "Achsen von Hand bewegen, Werkstück-Nullpunkt setzen.",
    _m("11", "PRESET", "Werkstück-Nullpunkt an der aktuellen Position setzen.", action="preset"),
    _m(
        "12",
        "INKREMENT",
        "Achsen schrittweise mit wählbarer Schrittweite verfahren.",
        _m("", "X −", "X in negativer Richtung", key=1, action="jog:0:-"),
        _m("", "X +", "X in positiver Richtung", key=2, action="jog:0:+"),
        _m("", "Y −", "Y in negativer Richtung", key=3, action="jog:1:-"),
        _m("", "Y +", "Y in positiver Richtung", key=4, action="jog:1:+"),
        _m("", "Z −", "Z in negativer Richtung", key=5, action="jog:2:-"),
        _m("", "Z +", "Z in positiver Richtung", key=6, action="jog:2:+"),
        _m("", "SCHRITT", "Schrittweite wechseln (0,01 / 0,1 / 1 / 10 / 100 mm)", key=7, action="jog_step"),
        _m("", "ZURÜCK", "zurück ins Hauptmenü 1", key=8, action=BACK),
        assumed=True,
    ),
    _m("13", "FAHREN", "Eine Koordinatenposition anfahren.", action="goto"),
    _m("15", "DIGITALI-\nSIEREN", "Arbeiten mit einem Digitalisiergerät.", available=False),
    _m("16", "SCHNITT-\nDATEN", "Schnittdaten (Drehzahl, Vorschub) berechnen.", action="cutting_data"),
    _m("17", "ACHSEN-\nWECHSEL", "Umschalten Drehachse C / Kippachse A.", available=False),
    _m(
        "18",
        "WEITER",
        "Untermenü 18",
        _m("181", "REF.\nPUNKT", "Referenzmarken anfahren.", available=False),
        _m("184", "AUTO-\nWZW", "Automatischer Werkzeugwechsler.", available=False),
        _m("185", "AUTO-\nPAL", "Automatischer Palettenwechsler.", available=False),
        _m("186", "SCHMIEREN\nKOPF", "Kopf schmieren.", available=False),
        _m("187", "SCHMIEREN\nACHSEN", "Achsen schmieren.", available=False),
        _m("188", "ZURÜCK", "zurück ins Hauptmenü 1", action=BACK),
    ),
)

MAIN_2 = _m(
    "2",
    "AUTOMATIK",
    "Gespeicherte Programme abarbeiten bzw. testen.",
    _m(
        "21",
        "TESTLAUF",
        "Programme testen; Untermenü 21.",
        _m(
            "211",
            "TESTLAUF",
            "Test im Folge-/Einzelsatz, mit/ohne Echtzeit, mit/ohne Grafik.",
            _m("2111", "FOLGESATZ\nEIN/AUS", "Folgesatz oder Einzelsatz", action="toggle_single"),
            _m("2113", "ECHTZEIT\nEIN/AUS", "Test in Echtzeit-Geschwindigkeit", action="toggle_realtime"),
            _m("2115", "ANWAHL\nVORLAUF", "Programm bzw. Startsatz anwählen", action="select"),
            _m("2116", "GRAFIK\nEIN/AUS", "Testgrafik ein/aus", action="toggle_graphics"),
            _m("2117", "START\nSTOP", "Testlauf starten/anhalten", action="start_stop"),
            _m("2118", "ZURÜCK", "zurück ins Untermenü 21", action=BACK),
        ),
        _m("212", "TESTLAUF\nMASCH KPL", "Test mit maximaler Vorschubgeschwindigkeit.", action="test_fast"),
        _m("213", "TESTLAUF\nMASCH TLW", "Test mit maximalem Vorschub ohne Werkzeugwechsel.", action="test_fast"),
        _m("215", "ANWAHL\nVORLAUF", "Programm bzw. Startsatz anwählen", action="select"),
        _m("216", "GRAFIK\nEIN/AUS", "Grafik ein/aus", action="toggle_graphics"),
        _m("218", "ZURÜCK", "zurück ins Hauptmenü 2", action=BACK),
    ),
    _m("24", "WERKZEUG\nMAGAZIN", "Magazinbelegung bei automatischem Werkzeugwechsler.", available=False, assumed=True),
    _m("25", "ANWAHL\nVORLAUF", "Programm anwählen, das abgearbeitet werden soll.", action="select"),
    _m("26", "GRAFIK\nEIN/AUS", "Grafik in Hauptbetriebsart 2 ein/aus.", action="toggle_graphics"),
    _m(
        "28",
        "WEITER",
        "Untermenü 28",
        _m("286", "SCHMIEREN\nKOPF", "Kopf während der Abarbeitung schmieren.", available=False),
        _m("287", "SCHMIEREN\nACHSEN", "Achsen während der Abarbeitung schmieren.", available=False),
        _m("288", "ZURÜCK", "zurück ins Hauptmenü 2", action=BACK),
    ),
)

MAIN_3 = _m(
    "3",
    "PROGRAMM",
    "Programme erstellen, ändern und verwalten.",
    _m("31", "HAUPT-\nPROGRAMM", "Aktuelles Hauptprogramm schreiben und editieren.", action="edit_main"),
    _m("32", "UNTER-\nPROGRAMM", "Unterprogramme (%*n) des aktuellen Programms.", action="edit_sub"),
    _m("33", "MAKRO-\nPROGRAMM", "Makroprogramme (%%n).", action="edit_macro"),
    _m("34", "WERK-\nZEUGE", "Werkzeugdaten des Programms (Radius, Länge, Technologie).", action="tools"),
    _m("35", "PARA-\nMETER", "Parameter des Programms.", action="parameters", assumed=True),
    _m("36", "GRAFIK\nEIN/AUS", "Grafik in Hauptbetriebsart 3 ein/aus.", action="toggle_graphics", assumed=True),
    _m("37", "KONTUR\nEIN/AUS", "Konturrechner ein/aus.", action="contour", assumed=True),
    _m(
        "38",
        "WEITER",
        "Untermenü 38",
        _m("381", "PROGRAMM\nVERWALTEN", "Gespeicherte Hauptprogramme und Programmdaten.", action="library"),
        _m("382", "MAKRO\nVERWALTEN", "Gespeicherte Makroprogramme.", action="library_macros"),
        _m("383", "WERKZEUGE\nALLGEMEIN", "Werkzeugdaten für alle Programme.", action="tools"),
        _m("384", "PARAMETER\nALLGEMEIN", "Parameter für alle Programme.", action="parameters"),
        _m("385", "NULL-\nPUNKT", "Nullpunktspeicher (G51.n) bearbeiten.", action="zero_points"),
        _m("386", "NOTIZEN", "Notizen schreiben und lesen.", action="notes"),
        _m(
            "387",
            "DATEN-\nTRANSFER",
            "Programme ein- und auslesen, Nachladebetrieb.",
            _m(
                "",
                "EINLESEN",
                "Programm in die Steuerung einlesen (in DeckelLink: Senden).",
                key=1,
                action="transfer_in",
            ),
            _m("", "AUSLESEN", "Programm ausgeben (in DeckelLink: Empfangen).", key=2, action="transfer_out"),
            _m(
                "",
                "NACHLADEN",
                "Nachladebetrieb ein/aus (in DeckelLink: Nachladebetrieb LSV2).",
                key=3,
                action="toggle_dripfeed",
            ),
            _m(
                "",
                "KONFIG\nI/O",
                "Schnittstellenparameter für bis zu 7 Partnergeräte.",
                key=6,
                action="config_io",
                assumed=True,
            ),
            _m("", "START", "Datenübertragung starten.", key=8, action="transfer_start"),
        ),
        _m("388", "ZURÜCK", "zurück ins Hauptmenü 3", action=BACK),
    ),
)

AUX = (
    _m(
        "MODE",
        "MODE",
        "Anzeige und Grafik konfigurieren.",
        _m("", "TEXT", "Text-/Achsanzeige", action="screen_text", key=1),
        _m("", "GRAFIK", "Grafikanzeige", action="screen_graphics", key=2),
        _m(
            "",
            "GRAFIK\nKONFIG",
            "Grafik konfigurieren",
            _m("", "1 EBENE", "Draufsicht XY", action="view:xy", key=1),
            _m("", "3 EBENEN", "nächste Ebene (XY, XZ, YZ)", action="view:cycle", key=2),
            _m("", "DRAHT-\nMODELL", "3D-Ansicht", action="view:iso", key=3),
            _m("", "DREHEN ◄", "3D-Ansicht drehen", action="rotate:-15", key=4),
            _m("", "DREHEN ►", "3D-Ansicht drehen", action="rotate:15", key=5),
            _m("", "ZURÜCK", "zurück", action=BACK, key=8),
            key=3,
        ),
        _m("", "ZOOM +", "vergrößern", action="zoom:in", key=4),
        _m("", "ZOOM −", "verkleinern / einpassen", action="zoom:out", key=5),
        _m("", "FRÄSER\nEIN/AUS", "Werkzeugdarstellung ein/aus", action="toggle_tool", key=6),
        _m("", "EILGANG\nEIN/AUS", "Eilgangdarstellung ein/aus", action="toggle_rapid", key=7),
        _m("", "BILD\nLÖSCHEN", "Grafik zurücksetzen", action="clear", key=8),
    ),
    _m(
        "INFO",
        "INFO",
        "Nur Einblick - kein Eingriff.",
        _m("", "PROGRAMM", "aktuelles Programm", action="screen_text", key=1),
        _m("", "PROGRAMM\nALLGEMEIN", "Programmverwaltung", action="library", key=2),
        _m("", "WERKZEUG-\nMAGAZIN", "Werkzeugwechsler-Verwaltung", available=False, key=3),
        _m("", "NOTIZEN", "Notizen", action="notes", key=4),
        _m("", "MEL-\nDUNGEN", "Fehlermeldungen", action="messages", key=5),
    ),
    _m(
        "HELP",
        "HELP",
        "Struktur der Steuerung und Befehlsübersicht.",
        _m("", "STRUKTUR", "Menüstruktur anzeigen", action="structure", key=1),
        _m("", "HANDFAHR", "Menüstruktur Handfahren", action="structure:1", key=2),
        _m("", "AUTOMATIK", "Menüstruktur Automatik", action="structure:2", key=3),
        _m("", "PROGRAMM", "Menüstruktur Programm", action="structure:3", key=4),
        _m("", "G0-G59", "G-Funktionen", action="reference", key=5),
        _m("", "G60-G95", "G-Funktionen", action="reference", key=6),
        _m("", "M0-M79", "M-Funktionen", action="reference", key=7),
    ),
    _m(
        "EDIT",
        "EDIT",
        "Programme umgestalten (nur in Hauptbetriebsart 3).",
        _m("", "SUCHEN", "Text suchen", action="find", key=1),
        _m("", "KOPIEREN", "Markierte Sätze kopieren", action="copy", key=2),
        _m("", "ERSETZEN", "Text ersetzen", action="replace", key=3),
        _m("", "LÖSCHEN", "Markierte Sätze löschen", action="delete", key=4),
        _m("", "SCHIEBEN", "Markierte Sätze ausschneiden (verschieben)", action="cut", key=5),
        _m("", "EINFÜGEN", "Kopierte/ausgeschnittene Sätze einfügen", action="paste", key=6),
    ),
    _m("GRAF", "GRAF", "Zwischen Grafik und Programmlisting umschalten.", action="toggle_screen"),
    _m(
        "DIA",
        "DIA",
        "Dialoggestützte Eingabe von Wegen und Zyklen (Hauptbetriebsart 3).",
        _m("", "G0\nPUNKT", "Eilgang", action="dia:G0", key=1),
        _m("", "G1\nGERADE", "Gerade", action="dia:G1", key=2),
        _m("", "G2\nKREIS RE", "Kreis rechts", action="dia:G2", key=3),
        _m("", "G3\nKREIS LI", "Kreis links", action="dia:G3", key=4),
        _m("", "BOHREN\nG81-G86", "Bohrzyklen", action="dia:DRILL", key=5),
        _m("", "FRAESEN\nG87", "Fräszyklen", action="dia:MILL", key=6),
        _m("", "MUSTER\nG89", "Punktemuster", action="dia:PATTERN", key=7),
    ),
)

MAIN_MODES = {1: MAIN_1, 2: MAIN_2, 3: MAIN_3}
AUX_MODES = {item.code: item for item in AUX}

# Address fields of the DIA input forms: (address, label)
DIA_FORMS: dict[str, tuple[str, tuple[tuple[str, str], ...]]] = {
    "G0": ("G0", (("X", "X"), ("Y", "Y"), ("Z", "Z"))),
    "G1": ("G1", (("X", "X"), ("Y", "Y"), ("Z", "Z"), ("F", "Vorschub F"))),
    "G2": (
        "G2",
        (
            ("X", "X Ziel"),
            ("Y", "Y Ziel"),
            ("Z", "Z Ziel"),
            ("I", "I Mittelpunkt"),
            ("J", "J Mittelpunkt"),
            ("F", "Vorschub F"),
        ),
    ),
    "G3": (
        "G3",
        (
            ("X", "X Ziel"),
            ("Y", "Y Ziel"),
            ("Z", "Z Ziel"),
            ("I", "I Mittelpunkt"),
            ("J", "J Mittelpunkt"),
            ("F", "Vorschub F"),
        ),
    ),
    "DRILL": (
        "G81",
        (
            ("G", "Zyklus (81-86)"),
            ("OA", "Oberfläche OA"),
            ("TA", "Tiefe absolut TA"),
            ("TI", "Tiefe inkr. TI"),
            ("SI", "Sicherheitsabstand SI"),
            ("EA", "Rückzugsebene EA"),
            ("MI", "Zustellung MI"),
            ("DI", "Abnahme DI"),
            ("ST", "Steigung ST"),
            ("F", "Vorschub F"),
        ),
    ),
    "MILL": (
        "G87.1",
        (
            ("G", "Zyklus (87.1/.2/.3/.4/.5)"),
            ("OA", "Oberfläche OA"),
            ("TI", "Tiefe TI"),
            ("SI", "Sicherheitsabstand SI"),
            ("EA", "Rückzugsebene EA"),
            ("LI", "Länge LI"),
            ("BI", "Breite BI"),
            ("D", "Durchmesser D"),
            ("R", "Eckenradius R"),
            ("AM", "Ausräummaß AM"),
            ("MI", "Zustellung MI"),
            ("ST", "Steigung ST"),
        ),
    ),
    "PATTERN": (
        "G89.1",
        (
            ("G", "Muster (89.1/89.2)"),
            ("LL", "Punktezahl LL"),
            ("LX", "Abstand LX / Mitte LX"),
            ("LY", "Abstand LY / Mitte LY"),
            ("BL", "Reihen BL"),
            ("BX", "Reihenabstand BX"),
            ("BY", "Reihenabstand BY"),
            ("R", "Teilkreisradius R"),
            ("AW", "Startwinkel AW"),
            ("L", "Punktezahl L (Teilkreis)"),
            ("P", "Ausführung P (Parameter-Nr.)"),
        ),
    ),
}


def build_block(kind: str, values: dict[str, str], number: int | None) -> str:
    """Assemble a block from DIA form values (empty fields are left out)."""
    default_g, fields = DIA_FORMS[kind]
    words: list[str] = []
    if number is not None:
        words.append(f"N{number}")
    g = values.get("G", "").strip() or default_g
    words.append(g if g.upper().startswith("G") else f"G{g}")
    for address, _label in fields:
        if address == "G":
            continue
        value = values.get(address, "").strip().replace(",", ".")
        if value:
            words.append(f"{address}{value}")
    return " ".join(words)


def structure_text(root: MenuItem | None = None) -> str:
    """Indented menu tree (HELP > STRUKTUR)."""
    roots = [root] if root is not None else [*MAIN_MODES.values(), *AUX]
    lines: list[str] = []
    for item in roots:
        for depth, node in item.walk():
            marker = "" if node.available else "  (Maschinenfunktion, nicht simuliert)"
            assumed = "  (Nummer abgeleitet)" if node.assumed else ""
            lines.append(f"{'    ' * depth}{node.title} - {node.description}{marker}{assumed}")
    return "\n".join(lines)
