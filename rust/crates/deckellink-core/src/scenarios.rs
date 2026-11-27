//! Predefined simulator scenarios (same ids, texts and settings as the Python version).

use crate::model::ProtocolMode;
use crate::transport::lsv2_sim::Lsv2SimConfig;
use crate::transport::simulated::SimulationConfig;

pub const SAMPLE_PROGRAM: &[u8] = b"(DECKELLINK SIMULATOR TESTPROGRAMM)\r\nN10 G90 G17\r\nN20 G00 X0 Y0 Z50\r\n\
N30 T1 M06\r\nN40 S1200 M03\r\nN50 G01 Z-1 F100\r\nN60 X40 F200\r\nN70 Y30\r\nN80 X0\r\nN90 Y0\r\n\
N100 G00 Z50 M05\r\nN110 M30\r\n";

pub const INVALID_PROGRAM: &[u8] =
    b"(PROGRAMM MIT UNG\xdcLTIGEN ZEICHEN)\r\nN10 G90\r\nN20 G00 X10 Y10 \x1b\r\nN30 M30\r\n";

pub const DEFAULT_SCENARIO_ID: &str = "send_ok";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenarioKind {
    Send,
    Receive,
    Both,
}

impl ScenarioKind {
    pub fn label(self) -> &'static str {
        match self {
            ScenarioKind::Send => "Senden",
            ScenarioKind::Receive => "Empfangen",
            ScenarioKind::Both => "Senden und Empfangen",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Scenario {
    pub id: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub kind: ScenarioKind,
    pub config: SimulationConfig,
    pub sample_program: &'static [u8],
    pub protocol: ProtocolMode,
    pub lsv2: Option<Lsv2SimConfig>,
}

fn plain(
    id: &'static str,
    title: &'static str,
    description: &'static str,
    kind: ScenarioKind,
    config: SimulationConfig,
) -> Scenario {
    Scenario {
        id,
        title,
        description,
        kind,
        config,
        sample_program: SAMPLE_PROGRAM,
        protocol: ProtocolMode::Plain,
        lsv2: None,
    }
}

fn lsv2(
    id: &'static str,
    title: &'static str,
    description: &'static str,
    kind: ScenarioKind,
    config: Lsv2SimConfig,
) -> Scenario {
    Scenario {
        id,
        title,
        description,
        kind,
        config: SimulationConfig::default(),
        sample_program: SAMPLE_PROGRAM,
        protocol: ProtocolMode::Lsv2,
        lsv2: Some(config),
    }
}

fn receiving(program: Vec<u8>) -> SimulationConfig {
    SimulationConfig { machine_program: program, ..SimulationConfig::default() }
}

/// All scenarios in the order of the Python version (without the replay scenario).
pub fn scenarios() -> Vec<Scenario> {
    use ScenarioKind::{Both, Receive, Send};
    let sample = SAMPLE_PROGRAM.to_vec();
    let mixed = String::from_utf8_lossy(SAMPLE_PROGRAM)
        .replace("N20 G00 X0 Y0 Z50\r\n", "N20 G00 X0 Y0 Z50\n")
        .replace("N40 S1200 M03\r\n", "N40 S1200 M03\r")
        + "\r\n";
    let mut nulls = vec![0u8; 20];
    nulls.extend_from_slice(SAMPLE_PROGRAM);
    vec![
        plain("send_ok", "Erfolgreiches Senden", "Die simulierte CNC nimmt alle Daten an, CTS und DSR sind aktiv.", Send, SimulationConfig::default()),
        plain("receive_ok", "Erfolgreicher Empfang", "Die simulierte CNC sendet ein vollständiges Programm mit M30.", Receive, receiving(sample.clone())),
        plain(
            "cts_off",
            "CTS bleibt aus",
            "Die Gegenstelle erteilt nie die Sendefreigabe (CTS). Bei Profilen mit CTS-Prüfung endet das Senden mit Handshake-Timeout.",
            Send,
            SimulationConfig { cts: false, ..SimulationConfig::default() },
        ),
        plain(
            "send_timeout",
            "Timeout während des Sendens",
            "Nach 100 Bytes nimmt die CNC keine Daten mehr an.",
            Send,
            SimulationConfig { write_stall_after_bytes: Some(100), ..SimulationConfig::default() },
        ),
        plain(
            "user_cancel",
            "Abbruch durch Benutzer",
            "Langsame Übertragung - brechen Sie manuell ab. Im automatischen Test wird nach 100 Bytes abgebrochen.",
            Both,
            SimulationConfig { machine_program: SAMPLE_PROGRAM.repeat(4), speed_factor: 0.25, ..SimulationConfig::default() },
        ),
        plain(
            "receive_no_m30",
            "Empfang ohne M30",
            "Die CNC sendet ein Programm ohne Programmende; der Empfang endet per Inaktivität und wird als unvollständig gekennzeichnet.",
            Receive,
            receiving(String::from_utf8_lossy(SAMPLE_PROGRAM).replace("N110 M30\r\n", "").into_bytes()),
        ),
        plain(
            "receive_nulls",
            "Empfang mit Nullbytes",
            "Im Datenstrom sind Nullbytes enthalten (z. B. Vorlauf/Leader).",
            Receive,
            SimulationConfig { machine_program: nulls, inject_null_every: 37, ..SimulationConfig::default() },
        ),
        plain(
            "disconnect",
            "Verbindungsabbruch während des Transfers",
            "Die Verbindung bricht nach 150 Bytes ab (Senden und Empfangen).",
            Both,
            SimulationConfig { machine_program: sample.clone(), disconnect_after_bytes: Some(150), ..SimulationConfig::default() },
        ),
        Scenario {
            sample_program: INVALID_PROGRAM,
            ..plain(
                "invalid_ascii_file",
                "Datei mit ungültigen ASCII-Zeichen",
                "Das Beispielprogramm enthält Umlaute und Steuerzeichen; die Prüfung sperrt die Übertragung.",
                Send,
                SimulationConfig::default(),
            )
        },
        plain(
            "slow",
            "Langsame Übertragung",
            "Übertragung mit einem Fünftel der Baudrate, empfangen in kleinen Paketen.",
            Both,
            SimulationConfig {
                machine_program: sample.clone(),
                speed_factor: 0.2,
                read_chunk_sizes: vec![1, 3, 7],
                ..SimulationConfig::default()
            },
        ),
        plain(
            "partial_io",
            "Partielle Reads und Writes",
            "Der Treiber nimmt höchstens 5 Bytes pro Schreibaufruf an und liefert variable Paketgrößen.",
            Both,
            SimulationConfig {
                machine_program: sample.clone(),
                max_write_chunk: 5,
                read_chunk_sizes: vec![1, 2, 13, 64],
                ..SimulationConfig::default()
            },
        ),
        plain(
            "receive_mixed_endings",
            "Empfang mit gemischten Zeilenenden und zusätzlichem CRLF",
            "Zeilenenden CR, LF und CRLF gemischt, nach M30 folgt ein zusätzliches CRLF.",
            Receive,
            receiving(mixed.into_bytes()),
        ),
        plain(
            "receive_corrupted",
            "Empfang mit beschädigten Bytes",
            "Einzelne Bytes werden verfälscht (simulierter Paritäts-/Störungsfehler).",
            Receive,
            SimulationConfig { machine_program: sample.clone(), corrupt_offsets: vec![5, 60, 120], ..SimulationConfig::default() },
        ),
        plain(
            "port_open_error",
            "Port nicht verfügbar",
            "Simuliert einen belegten Anschluss beim Öffnen.",
            Both,
            SimulationConfig { open_error: Some("busy".into()), ..SimulationConfig::default() },
        ),
        plain(
            "write_error",
            "Schreibfehler",
            "Nach 80 Bytes meldet der Treiber einen Schreibfehler.",
            Send,
            SimulationConfig { write_error_after_bytes: Some(80), ..SimulationConfig::default() },
        ),
        lsv2(
            "lsv2_ok",
            "LSV2: Erfolgreiche Übertragung",
            "LSV2-CNC quittiert jedes Telegramm sofort und sendet beim Empfang ein Programm. Nur mit LSV2-Profil wirksam.",
            Both,
            Lsv2SimConfig { program: sample.clone(), ..Lsv2SimConfig::default() },
        ),
        lsv2(
            "lsv2_dripfeed",
            "LSV2: Nachladebetrieb (Puffer 3 Sätze, 0,5 s je Satz)",
            "Die simulierte CNC arbeitet das Programm ab und quittiert erst, wenn im Satzpuffer Platz ist. Nur mit LSV2-Profil wirksam.",
            Send,
            Lsv2SimConfig { buffer_telegrams: 3, execution_s_per_telegram: 0.5, ..Lsv2SimConfig::default() },
        ),
        lsv2(
            "lsv2_nak",
            "LSV2: Prüfsummenfehler / NAK",
            "Die CNC antwortet auf Telegramm 2 und 5 einmal mit NAK, beim Senden der CNC ist Telegramm 3 beschädigt. Die Übertragung muss durch Wiederholung gelingen.",
            Both,
            Lsv2SimConfig {
                nak_on_telegrams: vec![2, 5],
                corrupt_bcc_on_sent_telegrams: vec![3],
                program: sample,
                ..Lsv2SimConfig::default()
            },
        ),
        lsv2(
            "lsv2_lost_ack",
            "LSV2: Quittung geht verloren",
            "Die Quittung für Telegramm 4 geht verloren; DeckelLink fragt mit ENQ nach, ohne das Telegramm doppelt zu senden.",
            Send,
            Lsv2SimConfig { drop_ack_on_telegrams: vec![4], ..Lsv2SimConfig::default() },
        ),
        lsv2(
            "lsv2_wack",
            "LSV2: CNC meldet 'Warten' (WACK)",
            "Die CNC antwortet bei Telegramm 2 und 3 mit WACK; DeckelLink wartet und fragt nach.",
            Send,
            Lsv2SimConfig { wack_on_telegrams: vec![2, 3], ..Lsv2SimConfig::default() },
        ),
        lsv2(
            "lsv2_cnc_abort",
            "LSV2: CNC bricht ab (EOT)",
            "Nach Telegramm 5 beendet die CNC die Verbindung mit EOT (z. B. Programmabbruch an der Steuerung).",
            Send,
            Lsv2SimConfig { eot_after_telegrams: Some(5), ..Lsv2SimConfig::default() },
        ),
        lsv2(
            "lsv2_not_ready",
            "LSV2: CNC nicht empfangsbereit",
            "Die CNC beantwortet ENQ nie - Verbindungsaufbau scheitert nach der Wartezeit.",
            Send,
            Lsv2SimConfig { never_ready: true, ..Lsv2SimConfig::default() },
        ),
        lsv2(
            "lsv2_disconnect",
            "LSV2: Verbindungsabbruch im Nachladebetrieb",
            "Nach Telegramm 4 bricht die Verbindung ab.",
            Send,
            Lsv2SimConfig {
                disconnect_after_telegrams: Some(4),
                buffer_telegrams: 2,
                execution_s_per_telegram: 0.2,
                ..Lsv2SimConfig::default()
            },
        ),
    ]
}

/// Scenario by id; unknown ids fall back to the default scenario.
pub fn get_scenario(id: &str) -> Scenario {
    let all = scenarios();
    let fallback = all.first().cloned();
    all.into_iter().find(|s| s.id == id).or(fallback).unwrap_or_else(|| {
        plain("send_ok", "Erfolgreiches Senden", "", ScenarioKind::Send, SimulationConfig::default())
    })
}

/// LSV2 simulator settings for any scenario (plain scenarios get sensible LSV2 defaults).
pub fn lsv2_config_for(scenario: &Scenario, receive: bool, drip_feed: bool) -> Lsv2SimConfig {
    let mut config = match &scenario.lsv2 {
        Some(config) => config.clone(),
        None => {
            let mut config =
                Lsv2SimConfig { open_error: scenario.config.open_error.clone(), ..Lsv2SimConfig::default() };
            if drip_feed {
                config.buffer_telegrams = 4;
                config.execution_s_per_telegram = 0.2;
            }
            config
        }
    };
    if receive && config.program.is_empty() {
        config.program = SAMPLE_PROGRAM.to_vec();
    }
    if !receive {
        // When the PC sends, the simulated CNC is the receiver.
        config.program = Vec::new();
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenario_list() {
        let all = scenarios();
        assert_eq!(all.len(), 23);
        let mut ids: Vec<&str> = all.iter().map(|s| s.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 23);
        assert_eq!(get_scenario("unbekannt").id, DEFAULT_SCENARIO_ID);
        let drip = lsv2_config_for(&get_scenario("send_ok"), false, true);
        assert_eq!((drip.buffer_telegrams, drip.program.len()), (4, 0));
        assert_eq!(lsv2_config_for(&get_scenario("lsv2_wack"), true, false).program, SAMPLE_PROGRAM);
    }
}
