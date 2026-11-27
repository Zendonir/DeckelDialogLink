//! GUI-independent presentation logic (unit-testable without widgets): gates, formatting,
//! loaded programs and the transport factory.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use deckellink_core::appdata::{AppConfig, ConnectionKind};
use deckellink_core::clock::{Clock, SystemClock};
use deckellink_core::model::Profile;
use deckellink_core::nc::analyzer::analyze;
use deckellink_core::nc::transformer::transform;
use deckellink_core::nc::{AnalysisResult, TransformResult};
use deckellink_core::parameters::sample_parameters;
use deckellink_core::scenarios::{get_scenario, lsv2_config_for, SAMPLE_PROGRAM};
use deckellink_core::transport::lsv2_sim::Lsv2SimulatedCnc;
use deckellink_core::transport::rfc2217::Rfc2217Transport;
use deckellink_core::transport::serial::SerialTransport;
use deckellink_core::transport::simulated::SimulatedTransport;
use deckellink_core::transport::tcp::TcpTransport;
use deckellink_core::transport::Transport;

pub fn format_bytes(count: usize) -> String {
    #[allow(clippy::cast_precision_loss)]
    let value = count as f64;
    if count < 1024 {
        format!("{count} B")
    } else if count < 1024 * 1024 {
        format!("{:.1} KB", value / 1024.0).replace('.', ",")
    } else {
        format!("{:.2} MB", value / (1024.0 * 1024.0)).replace('.', ",")
    }
}

pub fn format_duration(seconds: f64) -> String {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let total = if seconds.is_finite() && seconds > 0.0 { seconds as u64 } else { 0 };
    format!("{:02}:{:02}:{:02}", total / 3600, (total % 3600) / 60, total % 60)
}

/// Readable preview of transfer bytes (control characters visible), at most `max_lines` lines.
pub fn preview_text(data: &[u8], max_lines: usize) -> String {
    let text: String = data
        .iter()
        .map(|b| match *b {
            b'\r' | b'\n' | b'\t' => char::from(*b),
            b if b < 0x20 || b == 0x7f => '·',
            b if b.is_ascii() => char::from(b),
            _ => '\u{FFFD}',
        })
        .collect();
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normalized.lines().collect();
    let mut out = lines.iter().take(max_lines).copied().collect::<Vec<_>>().join("\n");
    if lines.len() > max_lines {
        out.push_str(&format!("\n... ({} weitere Zeilen)", lines.len() - max_lines));
    }
    out
}

/// Hex dump line for the terminal.
pub fn hex_line(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
}

/// ASCII view with control characters in angle brackets.
pub fn ascii_line(data: &[u8]) -> String {
    let mut out = String::new();
    for b in data {
        match *b {
            0x00 => out.push_str("<NUL>"),
            0x02 => out.push_str("<STX>"),
            0x03 => out.push_str("<ETX>"),
            0x04 => out.push_str("<EOT>"),
            0x05 => out.push_str("<ENQ>"),
            0x0A => out.push_str("<LF>"),
            0x0D => out.push_str("<CR>"),
            0x10 => out.push_str("<DLE>"),
            0x11 => out.push_str("<XON>"),
            0x13 => out.push_str("<XOFF>"),
            0x15 => out.push_str("<NAK>"),
            b if !(0x20..0x7f).contains(&b) => out.push_str(&format!("<{b:02X}>")),
            b => out.push(char::from(b)),
        }
    }
    out
}

/// A loaded NC file: original bytes (never modified), analysis and transfer data.
#[derive(Debug, Clone)]
pub struct LoadedProgram {
    pub path: PathBuf,
    pub original: Vec<u8>,
    pub analysis: AnalysisResult,
    pub payload: Option<TransformResult>,
    pub transform_error: Option<String>,
}

impl LoadedProgram {
    pub fn new(path: PathBuf, original: Vec<u8>, profile: &Profile, append_end_confirmed: bool) -> Self {
        let analysis = analyze(&original, &profile.transfer, Some(&profile.serial));
        let mut program = Self { path, original, analysis, payload: None, transform_error: None };
        program.retransform(profile, append_end_confirmed);
        program
    }

    pub fn retransform(&mut self, profile: &Profile, append_end_confirmed: bool) {
        self.payload = None;
        self.transform_error = None;
        if self.analysis.has_errors() {
            self.transform_error = Some("Prüffehler verhindern die Aufbereitung".into());
            return;
        }
        match transform(&self.original, &profile.transfer, append_end_confirmed) {
            Ok(payload) => self.payload = Some(payload),
            Err(err) => self.transform_error = Some(err.to_string()),
        }
    }

    pub fn file_name(&self) -> String {
        self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }
}

/// Whether an action may be started, with German reasons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gate {
    pub allowed: bool,
    pub reasons: Vec<String>,
}

impl Gate {
    fn from(reasons: Vec<String>) -> Self {
        Self { allowed: reasons.is_empty(), reasons }
    }

    pub fn reason_text(&self) -> String {
        self.reasons.join("; ")
    }
}

const NO_TARGET: &str = "Kein COM-Port gewählt und Simulator nicht aktiv";
const BUSY: &str = "Ein anderer Vorgang läuft";

pub fn evaluate_send_gate(
    program: Option<&LoadedProgram>,
    simulation: bool,
    target: &str,
    busy: bool,
    warnings_acknowledged: bool,
) -> Gate {
    let mut reasons = Vec::new();
    match program {
        None => reasons.push("Keine Datei geladen".into()),
        Some(program) => {
            if program.analysis.has_errors() {
                reasons.push("Prüfung enthält Fehler".into());
            }
            if program.payload.is_none() {
                reasons.push("Transferdaten konnten nicht erzeugt werden".into());
            }
            if program.analysis.has_warnings() && !warnings_acknowledged {
                reasons.push("Warnungen noch nicht bestätigt".into());
            }
        }
    }
    if !simulation && target.is_empty() {
        reasons.push(NO_TARGET.into());
    }
    if busy {
        reasons.push(BUSY.into());
    }
    Gate::from(reasons)
}

pub fn evaluate_receive_gate(simulation: bool, target: &str, busy: bool) -> Gate {
    let mut reasons = Vec::new();
    if !simulation && target.is_empty() {
        reasons.push(NO_TARGET.into());
    }
    if busy {
        reasons.push(BUSY.into());
    }
    Gate::from(reasons)
}

/// What a transport is created for (selects the simulator data).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Send,
    DripFeed,
    Receive,
    ParamRead,
    ParamWrite,
}

impl Purpose {
    fn receiving(self) -> bool {
        matches!(self, Purpose::Receive | Purpose::ParamRead)
    }
}

/// Connection target of the real connection ("" if not configured).
pub fn connection_target(config: &AppConfig) -> String {
    match config.connection_kind {
        ConnectionKind::Serial => config.port.trim().to_string(),
        ConnectionKind::Tcp | ConnectionKind::Rfc2217 => {
            let host = config.network_host.trim();
            if host.is_empty() {
                String::new()
            } else {
                format!("{host}:{}", config.network_port)
            }
        }
    }
}

/// Label of the port as shown in the header and stored in results.
pub fn port_label(config: &AppConfig) -> String {
    if config.simulation_mode {
        return "SIMULATOR".into();
    }
    real_port_label(config)
}

/// Label of the configured real connection, also while simulation mode is active.
pub fn real_port_label(config: &AppConfig) -> String {
    let target = connection_target(config);
    match config.connection_kind {
        ConnectionKind::Serial => target,
        ConnectionKind::Tcp => format!("TCP {target}"),
        ConnectionKind::Rfc2217 => format!("RFC2217 {target}"),
    }
}

/// Create the transport for `purpose`: the simulator in simulation mode, otherwise the configured connection.
pub fn make_transport(config: &AppConfig, profile: &Profile, purpose: Purpose) -> Box<dyn Transport> {
    let serial = profile.serial.clone();
    if config.simulation_mode {
        let clock: Arc<dyn Clock> = Arc::new(SystemClock::default());
        let scenario = get_scenario(&config.simulation_scenario);
        if profile.protocol.is_lsv2() {
            let mut lsv2 = lsv2_config_for(&scenario, purpose.receiving(), purpose == Purpose::DripFeed);
            if purpose == Purpose::ParamRead {
                lsv2.program = sample_parameters();
            }
            return Box::new(Lsv2SimulatedCnc::new(serial, profile.protocol.clone(), lsv2, clock));
        }
        let mut sim = scenario.config.clone();
        if purpose == Purpose::ParamRead {
            sim.machine_program = sample_parameters();
        }
        if purpose == Purpose::Receive && sim.machine_program.is_empty() {
            sim.machine_program = SAMPLE_PROGRAM.to_vec();
        }
        return Box::new(SimulatedTransport::new(serial, sim, clock));
    }
    match config.connection_kind {
        ConnectionKind::Serial => Box::new(SerialTransport::new(config.port.trim().to_string(), serial)),
        ConnectionKind::Tcp => Box::new(TcpTransport::new(config.network_host.trim(), config.network_port, serial)),
        ConnectionKind::Rfc2217 => {
            Box::new(Rfc2217Transport::new(config.network_host.trim(), config.network_port, serial))
        }
    }
}

/// File name for the simulator sample of a scenario.
pub fn sample_file(dir: &Path, scenario_id: &str) -> PathBuf {
    dir.join(format!("SIM_{}.nc", deckellink_core::storage::sanitize_stem(scenario_id, "sample")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckellink_core::profile::builtin_profiles;

    fn profile() -> Profile {
        builtin_profiles().0.into_iter().next().unwrap()
    }

    #[test]
    fn formatting() {
        assert_eq!(format_bytes(12), "12 B");
        assert_eq!(format_bytes(2048), "2,0 KB");
        assert_eq!(format_bytes(3 * 1024 * 1024), "3,00 MB");
        assert_eq!(format_duration(3725.9), "01:02:05");
        assert_eq!(format_duration(f64::NAN), "00:00:00");
        assert_eq!(ascii_line(b"A\r\n\x04\xff"), "A<CR><LF><EOT><FF>");
        assert_eq!(hex_line(b"\x10\x02"), "10 02");
        assert_eq!(preview_text(b"A\r\nB\r\nC", 2), "A\nB\n... (1 weitere Zeilen)");
    }

    #[test]
    fn send_gate() {
        let p = profile();
        let ok = LoadedProgram::new("a.nc".into(), b"N10 G0\r\nM30\r\n".to_vec(), &p, false);
        assert!(evaluate_send_gate(Some(&ok), true, "", false, false).allowed);
        let gate = evaluate_send_gate(Some(&ok), false, "", true, false);
        assert_eq!(gate.reasons.len(), 2);
        assert!(!evaluate_send_gate(None, true, "", false, true).allowed);
        let bad = LoadedProgram::new("b.nc".into(), b"N10 \x1b\r\nM30\r\n".to_vec(), &p, false);
        let gate = evaluate_send_gate(Some(&bad), true, "", false, true);
        assert!(gate.reasons.contains(&"Prüfung enthält Fehler".to_string()));
        let warn = LoadedProgram::new("c.nc".into(), b"N10 G0\r\n".to_vec(), &p, false);
        assert!(warn.analysis.has_warnings());
        assert!(!evaluate_send_gate(Some(&warn), true, "", false, false).allowed);
        assert!(evaluate_send_gate(Some(&warn), true, "", false, true).allowed);
        assert!(!evaluate_receive_gate(false, "", false).allowed);
        assert!(evaluate_receive_gate(false, "COM3", false).allowed);
    }

    #[test]
    fn targets_and_transports() {
        let p = profile();
        let mut config = AppConfig::default();
        assert_eq!(port_label(&config), "SIMULATOR");
        config.port = "COM7".into();
        assert_eq!(real_port_label(&config), "COM7");
        assert!(make_transport(&config, &p, Purpose::Receive).is_simulated());
        config.simulation_mode = false;
        config.port = "COM3".into();
        assert_eq!(port_label(&config), "COM3");
        assert!(!make_transport(&config, &p, Purpose::Send).is_simulated());
        config.connection_kind = ConnectionKind::Rfc2217;
        assert_eq!(connection_target(&config), "");
        config.network_host = "10.0.0.5".into();
        assert_eq!(port_label(&config), "RFC2217 10.0.0.5:4001");
    }
}
