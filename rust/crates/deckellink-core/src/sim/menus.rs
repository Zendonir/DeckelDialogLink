//! Menu structure of the DIALOG 11 as reproduced in the simulator.
//!
//! Main modes, sub-modes and auxiliary modes with their softkey numbers follow the
//! structure overview of the training documentation (module 4). Codes marked
//! `assumed` are not legible there and were inferred from their neighbours.
//! Descriptions are DeckelLink's own short texts. `available == false` marks machine
//! functions the simulator does not reproduce (they only show an explanation).

use std::sync::OnceLock;

pub const BACK: &str = "back";
pub const NOT_SIMULATED: &str = "Maschinenfunktion - im Simulator nicht nachgebildet.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub code: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub action: &'static str,
    pub children: Vec<MenuItem>,
    pub available: bool,
    pub assumed: bool,
    /// Softkey position 1..8 (0 = derived from the last digit of the code).
    pub key: usize,
}

fn m(code: &'static str, label: &'static str, description: &'static str) -> MenuItem {
    MenuItem { code, label, description, action: "", children: Vec::new(), available: true, assumed: false, key: 0 }
}

impl MenuItem {
    fn act(mut self, action: &'static str) -> Self {
        self.action = action;
        self
    }

    fn kids(mut self, children: Vec<MenuItem>) -> Self {
        self.children = children;
        self
    }

    fn unavailable(mut self) -> Self {
        self.available = false;
        self
    }

    fn assume(mut self) -> Self {
        self.assumed = true;
        self
    }

    fn on(mut self, key: usize) -> Self {
        self.key = key;
        self
    }

    pub fn softkey(&self) -> usize {
        if self.key != 0 {
            return self.key;
        }
        self.code.chars().last().and_then(|c| c.to_digit(10)).map_or(0, |d| d as usize)
    }

    pub fn is_menu(&self) -> bool {
        !self.children.is_empty()
    }

    /// Label on one line (softkey labels may contain line breaks).
    pub fn flat_label(&self) -> String {
        self.label.replace("-\n", "-").replace('\n', " ")
    }

    pub fn title(&self) -> String {
        if self.code.is_empty() || self.code == self.label {
            self.flat_label()
        } else {
            format!("{} {}", self.code, self.flat_label()).trim().to_string()
        }
    }

    /// Children placed on softkeys F1..F8.
    pub fn slots(&self) -> [Option<&MenuItem>; 8] {
        let mut slots: [Option<&MenuItem>; 8] = [None; 8];
        for child in &self.children {
            let position = child.softkey();
            if let Some(slot) = position.checked_sub(1).and_then(|i| slots.get_mut(i)) {
                if slot.is_none() {
                    *slot = Some(child);
                }
            }
        }
        slots
    }

    pub fn find(&self, code: &str) -> Option<&MenuItem> {
        if self.code == code {
            return Some(self);
        }
        self.children.iter().find_map(|c| c.find(code))
    }

    pub fn walk(&self, depth: usize) -> Vec<(usize, &MenuItem)> {
        let mut out = vec![(depth, self)];
        for child in &self.children {
            out.extend(child.walk(depth + 1));
        }
        out
    }
}

fn main_1() -> MenuItem {
    m("1", "HANDFAHREN", "Achsen von Hand bewegen, Werkstück-Nullpunkt setzen.").kids(vec![
        m("11", "PRESET", "Werkstück-Nullpunkt an der aktuellen Position setzen.").act("preset"),
        m("12", "INKREMENT", "Achsen schrittweise mit wählbarer Schrittweite verfahren.")
            .kids(vec![
                m("", "X −", "X in negativer Richtung").on(1).act("jog:0:-"),
                m("", "X +", "X in positiver Richtung").on(2).act("jog:0:+"),
                m("", "Y −", "Y in negativer Richtung").on(3).act("jog:1:-"),
                m("", "Y +", "Y in positiver Richtung").on(4).act("jog:1:+"),
                m("", "Z −", "Z in negativer Richtung").on(5).act("jog:2:-"),
                m("", "Z +", "Z in positiver Richtung").on(6).act("jog:2:+"),
                m("", "SCHRITT", "Schrittweite wechseln (0,01 / 0,1 / 1 / 10 / 100 mm)").on(7).act("jog_step"),
                m("", "ZURÜCK", "zurück ins Hauptmenü 1").on(8).act(BACK),
            ])
            .assume(),
        m("13", "FAHREN", "Eine Koordinatenposition anfahren.").act("goto"),
        m("15", "DIGITALI-\nSIEREN", "Arbeiten mit einem Digitalisiergerät.").unavailable(),
        m("16", "SCHNITT-\nDATEN", "Schnittdaten (Drehzahl, Vorschub) berechnen.").act("cutting_data"),
        m("17", "ACHSEN-\nWECHSEL", "Umschalten Drehachse C / Kippachse A.").unavailable(),
        m("18", "WEITER", "Untermenü 18").kids(vec![
            m("181", "REF.\nPUNKT", "Referenzmarken anfahren.").unavailable(),
            m("184", "AUTO-\nWZW", "Automatischer Werkzeugwechsler.").unavailable(),
            m("185", "AUTO-\nPAL", "Automatischer Palettenwechsler.").unavailable(),
            m("186", "SCHMIEREN\nKOPF", "Kopf schmieren.").unavailable(),
            m("187", "SCHMIEREN\nACHSEN", "Achsen schmieren.").unavailable(),
            m("188", "ZURÜCK", "zurück ins Hauptmenü 1").act(BACK),
        ]),
    ])
}

fn main_2() -> MenuItem {
    m("2", "AUTOMATIK", "Gespeicherte Programme abarbeiten bzw. testen.").kids(vec![
        m("21", "TESTLAUF", "Programme testen; Untermenü 21.").kids(vec![
            m("211", "TESTLAUF", "Test im Folge-/Einzelsatz, mit/ohne Echtzeit, mit/ohne Grafik.").kids(vec![
                m("2111", "FOLGESATZ\nEIN/AUS", "Folgesatz oder Einzelsatz").act("toggle_single"),
                m("2113", "ECHTZEIT\nEIN/AUS", "Test in Echtzeit-Geschwindigkeit").act("toggle_realtime"),
                m("2115", "ANWAHL\nVORLAUF", "Programm bzw. Startsatz anwählen").act("select"),
                m("2116", "GRAFIK\nEIN/AUS", "Testgrafik ein/aus").act("toggle_graphics"),
                m("2117", "START\nSTOP", "Testlauf starten/anhalten").act("start_stop"),
                m("2118", "ZURÜCK", "zurück ins Untermenü 21").act(BACK),
            ]),
            m("212", "TESTLAUF\nMASCH KPL", "Test mit maximaler Vorschubgeschwindigkeit.").act("test_fast"),
            m("213", "TESTLAUF\nMASCH TLW", "Test mit maximalem Vorschub ohne Werkzeugwechsel.").act("test_fast"),
            m("215", "ANWAHL\nVORLAUF", "Programm bzw. Startsatz anwählen").act("select"),
            m("216", "GRAFIK\nEIN/AUS", "Grafik ein/aus").act("toggle_graphics"),
            m("218", "ZURÜCK", "zurück ins Hauptmenü 2").act(BACK),
        ]),
        m("24", "WERKZEUG\nMAGAZIN", "Magazinbelegung bei automatischem Werkzeugwechsler.").unavailable().assume(),
        m("25", "ANWAHL\nVORLAUF", "Programm anwählen, das abgearbeitet werden soll.").act("select"),
        m("26", "GRAFIK\nEIN/AUS", "Grafik in Hauptbetriebsart 2 ein/aus.").act("toggle_graphics"),
        m("28", "WEITER", "Untermenü 28").kids(vec![
            m("286", "SCHMIEREN\nKOPF", "Kopf während der Abarbeitung schmieren.").unavailable(),
            m("287", "SCHMIEREN\nACHSEN", "Achsen während der Abarbeitung schmieren.").unavailable(),
            m("288", "ZURÜCK", "zurück ins Hauptmenü 2").act(BACK),
        ]),
    ])
}

fn main_3() -> MenuItem {
    m("3", "PROGRAMM", "Programme erstellen, ändern und verwalten.").kids(vec![
        m("31", "HAUPT-\nPROGRAMM", "Aktuelles Hauptprogramm schreiben und editieren.").act("edit_main"),
        m("32", "UNTER-\nPROGRAMM", "Unterprogramme (%*n) des aktuellen Programms.").act("edit_sub"),
        m("33", "MAKRO-\nPROGRAMM", "Makroprogramme (%%n).").act("edit_macro"),
        m("34", "WERK-\nZEUGE", "Werkzeugdaten des Programms (Radius, Länge, Technologie).").act("tools"),
        m("35", "PARA-\nMETER", "Parameter des Programms.").act("parameters").assume(),
        m("36", "GRAFIK\nEIN/AUS", "Grafik in Hauptbetriebsart 3 ein/aus.").act("toggle_graphics").assume(),
        m("37", "KONTUR\nEIN/AUS", "Konturrechner ein/aus.").act("contour").assume(),
        m("38", "WEITER", "Untermenü 38").kids(vec![
            m("381", "PROGRAMM\nVERWALTEN", "Gespeicherte Hauptprogramme und Programmdaten.").act("library"),
            m("382", "MAKRO\nVERWALTEN", "Gespeicherte Makroprogramme.").act("library_macros"),
            m("383", "WERKZEUGE\nALLGEMEIN", "Werkzeugdaten für alle Programme.").act("tools"),
            m("384", "PARAMETER\nALLGEMEIN", "Parameter für alle Programme.").act("parameters"),
            m("385", "NULL-\nPUNKT", "Nullpunktspeicher (G51.n) bearbeiten.").act("zero_points"),
            m("386", "NOTIZEN", "Notizen schreiben und lesen.").act("notes"),
            m("387", "DATEN-\nTRANSFER", "Programme ein- und auslesen, Nachladebetrieb.").kids(vec![
                m("", "EINLESEN", "Programm in die Steuerung einlesen (in DeckelLink: Senden).")
                    .on(1)
                    .act("transfer_in"),
                m("", "AUSLESEN", "Programm ausgeben (in DeckelLink: Empfangen).").on(2).act("transfer_out"),
                m("", "NACHLADEN", "Nachladebetrieb ein/aus (in DeckelLink: Nachladebetrieb LSV2).")
                    .on(3)
                    .act("toggle_dripfeed"),
                m("", "KONFIG\nI/O", "Schnittstellenparameter für bis zu 7 Partnergeräte.")
                    .on(6)
                    .act("config_io")
                    .assume(),
                m("", "START", "Datenübertragung starten.").on(8).act("transfer_start"),
            ]),
            m("388", "ZURÜCK", "zurück ins Hauptmenü 3").act(BACK),
        ]),
    ])
}

fn aux() -> Vec<MenuItem> {
    vec![
        m("MODE", "MODE", "Anzeige und Grafik konfigurieren.").kids(vec![
            m("", "TEXT", "Text-/Achsanzeige").act("screen_text").on(1),
            m("", "GRAFIK", "Grafikanzeige").act("screen_graphics").on(2),
            m("", "GRAFIK\nKONFIG", "Grafik konfigurieren")
                .kids(vec![
                    m("", "1 EBENE", "Draufsicht XY").act("view:xy").on(1),
                    m("", "3 EBENEN", "nächste Ebene (XY, XZ, YZ)").act("view:cycle").on(2),
                    m("", "DRAHT-\nMODELL", "3D-Ansicht").act("view:iso").on(3),
                    m("", "DREHEN ◄", "3D-Ansicht drehen").act("rotate:-15").on(4),
                    m("", "DREHEN ►", "3D-Ansicht drehen").act("rotate:15").on(5),
                    m("", "ZURÜCK", "zurück").act(BACK).on(8),
                ])
                .on(3),
            m("", "ZOOM +", "vergrößern").act("zoom:in").on(4),
            m("", "ZOOM −", "verkleinern / einpassen").act("zoom:out").on(5),
            m("", "FRÄSER\nEIN/AUS", "Werkzeugdarstellung ein/aus").act("toggle_tool").on(6),
            m("", "EILGANG\nEIN/AUS", "Eilgangdarstellung ein/aus").act("toggle_rapid").on(7),
            m("", "BILD\nLÖSCHEN", "Grafik zurücksetzen").act("clear").on(8),
        ]),
        m("INFO", "INFO", "Nur Einblick - kein Eingriff.").kids(vec![
            m("", "PROGRAMM", "aktuelles Programm").act("screen_text").on(1),
            m("", "PROGRAMM\nALLGEMEIN", "Programmverwaltung").act("library").on(2),
            m("", "WERKZEUG-\nMAGAZIN", "Werkzeugwechsler-Verwaltung").unavailable().on(3),
            m("", "NOTIZEN", "Notizen").act("notes").on(4),
            m("", "MEL-\nDUNGEN", "Fehlermeldungen").act("messages").on(5),
        ]),
        m("HELP", "HELP", "Struktur der Steuerung und Befehlsübersicht.").kids(vec![
            m("", "STRUKTUR", "Menüstruktur anzeigen").act("structure").on(1),
            m("", "HANDFAHR", "Menüstruktur Handfahren").act("structure:1").on(2),
            m("", "AUTOMATIK", "Menüstruktur Automatik").act("structure:2").on(3),
            m("", "PROGRAMM", "Menüstruktur Programm").act("structure:3").on(4),
            m("", "G0-G59", "G-Funktionen").act("reference").on(5),
            m("", "G60-G95", "G-Funktionen").act("reference").on(6),
            m("", "M0-M79", "M-Funktionen").act("reference").on(7),
        ]),
        m("EDIT", "EDIT", "Programme umgestalten (nur in Hauptbetriebsart 3).").kids(vec![
            m("", "SUCHEN", "Text suchen").act("find").on(1),
            m("", "KOPIEREN", "Markierte Sätze kopieren").act("copy").on(2),
            m("", "ERSETZEN", "Text ersetzen").act("replace").on(3),
            m("", "LÖSCHEN", "Markierte Sätze löschen").act("delete").on(4),
            m("", "SCHIEBEN", "Markierte Sätze ausschneiden (verschieben)").act("cut").on(5),
            m("", "EINFÜGEN", "Kopierte/ausgeschnittene Sätze einfügen").act("paste").on(6),
        ]),
        m("GRAF", "GRAF", "Zwischen Grafik und Programmlisting umschalten.").act("toggle_screen"),
        m("DIA", "DIA", "Dialoggestützte Eingabe von Wegen und Zyklen (Hauptbetriebsart 3).").kids(vec![
            m("", "G0\nPUNKT", "Eilgang").act("dia:G0").on(1),
            m("", "G1\nGERADE", "Gerade").act("dia:G1").on(2),
            m("", "G2\nKREIS RE", "Kreis rechts").act("dia:G2").on(3),
            m("", "G3\nKREIS LI", "Kreis links").act("dia:G3").on(4),
            m("", "BOHREN\nG81-G86", "Bohrzyklen").act("dia:DRILL").on(5),
            m("", "FRAESEN\nG87", "Fräszyklen").act("dia:MILL").on(6),
            m("", "MUSTER\nG89", "Punktemuster").act("dia:PATTERN").on(7),
        ]),
    ]
}

/// Main modes 1, 2, 3 (index 0..=2).
pub fn main_modes() -> &'static [MenuItem; 3] {
    static MODES: OnceLock<[MenuItem; 3]> = OnceLock::new();
    MODES.get_or_init(|| [main_1(), main_2(), main_3()])
}

pub fn main_mode(number: usize) -> Option<&'static MenuItem> {
    number.checked_sub(1).and_then(|i| main_modes().get(i))
}

/// Auxiliary modes MODE, INFO, HELP, EDIT, GRAF, DIA.
pub fn aux_modes() -> &'static [MenuItem] {
    static AUX: OnceLock<Vec<MenuItem>> = OnceLock::new();
    AUX.get_or_init(aux)
}

pub fn aux_mode(code: &str) -> Option<&'static MenuItem> {
    aux_modes().iter().find(|m| m.code == code)
}

/// Address fields of a DIA input form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiaForm {
    pub kind: &'static str,
    pub default_g: &'static str,
    /// (address, label)
    pub fields: &'static [(&'static str, &'static str)],
}

const TARGET_FIELDS: &[(&str, &str)] = &[
    ("X", "X Ziel"),
    ("Y", "Y Ziel"),
    ("Z", "Z Ziel"),
    ("I", "I Mittelpunkt"),
    ("J", "J Mittelpunkt"),
    ("F", "Vorschub F"),
];

pub const DIA_FORMS: &[DiaForm] = &[
    DiaForm { kind: "G0", default_g: "G0", fields: &[("X", "X"), ("Y", "Y"), ("Z", "Z")] },
    DiaForm { kind: "G1", default_g: "G1", fields: &[("X", "X"), ("Y", "Y"), ("Z", "Z"), ("F", "Vorschub F")] },
    DiaForm { kind: "G2", default_g: "G2", fields: TARGET_FIELDS },
    DiaForm { kind: "G3", default_g: "G3", fields: TARGET_FIELDS },
    DiaForm {
        kind: "DRILL",
        default_g: "G81",
        fields: &[
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
        ],
    },
    DiaForm {
        kind: "MILL",
        default_g: "G87.1",
        fields: &[
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
        ],
    },
    DiaForm {
        kind: "PATTERN",
        default_g: "G89.1",
        fields: &[
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
        ],
    },
];

pub fn dia_form(kind: &str) -> Option<&'static DiaForm> {
    DIA_FORMS.iter().find(|f| f.kind == kind)
}

/// Assemble a block from DIA form values (empty fields are left out).
pub fn build_block(form: &DiaForm, value: &dyn Fn(&str) -> String, number: Option<i64>) -> String {
    let mut words: Vec<String> = Vec::new();
    if let Some(number) = number {
        words.push(format!("N{number}"));
    }
    let g = value("G").trim().to_string();
    let g = if g.is_empty() { form.default_g.to_string() } else { g };
    words.push(if g.to_uppercase().starts_with('G') { g } else { format!("G{g}") });
    for (address, _label) in form.fields {
        if *address == "G" {
            continue;
        }
        let text = value(address).trim().replace(',', ".");
        if !text.is_empty() {
            words.push(format!("{address}{text}"));
        }
    }
    words.join(" ")
}

/// Indented menu tree (HELP > STRUKTUR).
pub fn structure_text(root: Option<&MenuItem>) -> String {
    let roots: Vec<&MenuItem> = match root {
        Some(root) => vec![root],
        None => main_modes().iter().chain(aux_modes().iter()).collect(),
    };
    let mut lines = Vec::new();
    for item in roots {
        for (depth, node) in item.walk(0) {
            let marker = if node.available { "" } else { "  (Maschinenfunktion, nicht simuliert)" };
            let assumed = if node.assumed { "  (Nummer abgeleitet)" } else { "" };
            lines.push(format!("{}{} - {}{marker}{assumed}", "    ".repeat(depth), node.title(), node.description));
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_slots_and_blocks() {
        let main3 = main_mode(3).unwrap();
        let slots = main3.slots();
        assert_eq!(slots[0].map(|m| m.code), Some("31"));
        assert_eq!(slots[7].map(|m| m.code), Some("38"));
        assert!(main3.find("387").is_some_and(MenuItem::is_menu));
        assert_eq!(aux_mode("DIA").map(|m| m.slots()[6].map(|s| s.action)), Some(Some("dia:PATTERN")));
        let form = dia_form("G1").unwrap();
        let values = |a: &str| match a {
            "X" => "10,5".to_string(),
            "F" => " 200 ".to_string(),
            _ => String::new(),
        };
        assert_eq!(build_block(form, &values, Some(40)), "N40 G1 X10.5 F200");
        assert!(structure_text(None).contains("387 DATEN-TRANSFER - Programme ein- und auslesen"));
    }
}
