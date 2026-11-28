//! Transport abstraction: real serial port, RS-232/Ethernet adapter (TCP), simulator, loopback.
//!
//! The GUI never talks to a port directly - only through [`Transport`].

pub mod loopback;
pub mod lsv2_sim;
pub mod rfc2217;
pub mod serial;
pub mod simulated;
pub mod tcp;

use crate::clock::CancelToken;
use crate::error::UserFacingError;

/// RS-232 line states. `None` means the adapter/driver cannot report it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModemStatus {
    pub cts: Option<bool>,
    pub dsr: Option<bool>,
    pub dcd: Option<bool>,
    pub ri: Option<bool>,
    pub rts: Option<bool>,
    pub dtr: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Busy,
    NotFound,
    NetworkUnreachable,
    Configuration,
    Open,
    HandshakeTimeout,
    Timeout,
    ConnectionLost,
    Write,
    Read,
    /// LSV2: the CNC did not become ready (no DLE 0 on ENQ).
    Lsv2LinkTimeout,
    /// LSV2: a telegram was not acknowledged after all retries.
    Lsv2AckTimeout,
    /// LSV2: too many NAKs / repetitions.
    Lsv2RetryExceeded,
    /// LSV2: the CNC ended the transfer (EOT or RVI).
    CncAbort,
    /// LSV2: other protocol failure.
    Lsv2Protocol,
}

/// A transport failure with the affected port.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct TransportError {
    pub kind: ErrorKind,
    pub message: String,
    pub port: String,
    /// Missing signals for handshake timeouts ("CTS", "DSR").
    pub missing: Vec<String>,
}

impl TransportError {
    pub fn new(kind: ErrorKind, message: impl Into<String>, port: impl Into<String>) -> Self {
        Self { kind, message: message.into(), port: port.into(), missing: Vec::new() }
    }
}

/// Byte transport to the CNC.
pub trait Transport: Send + std::fmt::Debug {
    fn name(&self) -> String;
    fn is_open(&self) -> bool;
    fn is_simulated(&self) -> bool;
    fn open(&mut self) -> Result<(), TransportError>;
    fn close(&mut self);
    /// Write bytes; may write fewer than given. Returns 0 if cancelled.
    fn write(&mut self, data: &[u8], cancel: &CancelToken) -> Result<usize, TransportError>;
    /// Read up to `max` bytes; returns an empty vector after the read timeout or on cancel.
    fn read(&mut self, max: usize, cancel: &CancelToken) -> Result<Vec<u8>, TransportError>;
    /// Bytes still in the driver output buffer.
    fn out_waiting(&mut self) -> usize;
    fn modem_status(&mut self) -> ModemStatus;
    fn set_control_lines(&mut self, rts: Option<bool>, dtr: Option<bool>) -> Result<(), TransportError>;
    fn reset_input(&mut self);
}

const PORT_OPEN_CAUSES: &[&str] = &[
    "Der Anschluss wird bereits von einem anderen Programm verwendet.",
    "Der USB-RS-232-Adapter wurde getrennt.",
    "Der Gerätetreiber ist nicht installiert.",
    "Der ausgewählte COM-Port existiert nicht mehr.",
];

/// German error description with probable causes (same texts as the Python version).
pub fn describe(err: &TransportError, operation: &str, profile_id: &str) -> UserFacingError {
    let port = if err.port.is_empty() { "Der Port".to_string() } else { err.port.clone() };
    let (title, causes): (String, Vec<String>) = match err.kind {
        ErrorKind::Busy => (
            format!("{port} konnte nicht geöffnet werden - der Anschluss ist belegt."),
            vec![
                "Der Anschluss wird bereits von einem anderen Programm verwendet (z. B. Terminalprogramm, DNC-Software)."
                    .into(),
                "Eine frühere DeckelLink-Instanz läuft noch.".into(),
                "Fehlende Zugriffsrechte auf den Anschluss.".into(),
            ],
        ),
        ErrorKind::NotFound => (
            format!("{port} wurde nicht gefunden."),
            vec![
                "Der USB-RS-232-Adapter wurde getrennt.".into(),
                "Der ausgewählte COM-Port existiert nicht mehr (andere COM-Nummer nach Neustecken).".into(),
                "Der Gerätetreiber ist nicht installiert.".into(),
            ],
        ),
        ErrorKind::NetworkUnreachable => (
            format!("Der Netzwerkadapter {port} ist nicht erreichbar."),
            vec![
                "IP-Adresse oder TCP-Port falsch (Adapter-Konfiguration prüfen).".into(),
                "Adapter ausgeschaltet, nicht im selben Netz oder Netzwerkkabel getrennt.".into(),
                "Adapter nicht im Modus 'TCP-Server' bzw. RFC 2217.".into(),
                "Eine andere Anwendung ist bereits mit dem Adapter verbunden (oft nur eine Verbindung erlaubt).".into(),
                "Firewall blockiert die Verbindung.".into(),
            ],
        ),
        ErrorKind::Configuration => {
            ("Die Einstellungen passen nicht zur gewählten Verbindungsart.".into(), vec![err.message.clone()])
        }
        ErrorKind::Open => (
            format!("{port} konnte nicht geöffnet werden."),
            PORT_OPEN_CAUSES.iter().map(|s| (*s).to_string()).collect(),
        ),
        ErrorKind::HandshakeTimeout => {
            let signals = if err.missing.is_empty() { "CTS".to_string() } else { err.missing.join("/") };
            (
                format!("Die Gegenstelle hat die Sendefreigabe ({signals}) nicht erteilt."),
                vec![
                    "Die CNC ist nicht empfangsbereit (Einlesen an der Steuerung nicht gestartet).".into(),
                    format!("Das Kabel führt die Leitung {signals} nicht oder ist anders belegt."),
                    "Das gewählte Profil passt nicht zur Handshake-Einstellung der Steuerung.".into(),
                    "Der USB-RS-232-Adapter unterstützt die Modemleitungen nicht.".into(),
                ],
            )
        }
        ErrorKind::Timeout => (
            "Zeitüberschreitung bei der Übertragung.".into(),
            vec![
                "Die Steuerung hat den Empfang angehalten (Handshake blockiert).".into(),
                "Die CNC ist nicht mehr empfangsbereit oder der Speicher ist voll.".into(),
                "Falsche Baudrate oder falsches Datenformat.".into(),
                "Kabel oder Adapter defekt.".into(),
            ],
        ),
        ErrorKind::ConnectionLost => (
            "Die Verbindung wurde während der Übertragung unterbrochen.".into(),
            vec![
                "Der USB-RS-232-Adapter wurde getrennt oder hat die Verbindung verloren.".into(),
                "Das Kabel wurde gelöst.".into(),
                "Der Treiber hat den Anschluss zurückgesetzt (Energiesparmodus des USB-Ports).".into(),
            ],
        ),
        ErrorKind::Write => (
            "Beim Senden ist ein Schreibfehler aufgetreten.".into(),
            vec!["Adapter- oder Treiberfehler.".into(), "Die Verbindung wurde unterbrochen.".into()],
        ),
        ErrorKind::Read => (
            "Beim Empfangen ist ein Lesefehler aufgetreten.".into(),
            vec!["Adapter- oder Treiberfehler.".into(), "Die Verbindung wurde unterbrochen.".into()],
        ),
        ErrorKind::Lsv2LinkTimeout => (
            "Die CNC hat die LSV2-Verbindung nicht angenommen (keine Quittung DLE 0 auf ENQ).".into(),
            vec![
                "Die Steuerung ist nicht im Empfangs- bzw. Nachladebetrieb (an der CNC starten).".into(),
                "Die Steuerung erwartet kein LSV2-Protokoll (Profil 'ohne Protokoll' verwenden).".into(),
                "Baudrate, Parität oder Datenbits stimmen nicht - Zeichen werden nicht erkannt.".into(),
                "Kabel/Adapter: RX/TX vertauscht oder nicht verbunden.".into(),
            ],
        ),
        ErrorKind::Lsv2AckTimeout => (
            "Die CNC hat ein Telegramm nicht quittiert.".into(),
            vec![
                "Die Steuerung hat den Empfang beendet oder wurde angehalten.".into(),
                "Im Nachladebetrieb: Quittungs-Timeout kürzer als die Bearbeitungszeit eines Satzes.".into(),
                "Telegrammaufbau (DLE-Transparenz, BCC) passt nicht zur Steuerung.".into(),
            ],
        ),
        ErrorKind::Lsv2RetryExceeded => (
            "LSV2-Übertragung nach mehreren Wiederholungen abgebrochen (NAK).".into(),
            vec![
                "Prüfsumme (BCC) wird von der Steuerung anders gebildet - BCC-Modus im Profil prüfen.".into(),
                "Störungen auf der Leitung, falsche Parität.".into(),
                "Telegramm zu lang für die Steuerung.".into(),
            ],
        ),
        ErrorKind::CncAbort => (
            format!("Die CNC hat die Übertragung beendet: {}", err.message),
            vec![
                "Bediener hat an der Steuerung abgebrochen oder das Programm gestoppt.".into(),
                "Die Steuerung hat einen Fehler im Datenstrom erkannt.".into(),
                "Speicher der Steuerung voll.".into(),
            ],
        ),
        ErrorKind::Lsv2Protocol => ("LSV2-Protokollfehler.".into(), vec![err.message.clone()]),
    };
    let mut technical = format!("{:?}: {}", err.kind, err.message);
    let context: Vec<String> = [
        (!operation.is_empty()).then(|| format!("Vorgang: {operation}")),
        (!err.port.is_empty()).then(|| format!("Port: {}", err.port)),
        (!profile_id.is_empty()).then(|| format!("Profil: {profile_id}")),
    ]
    .into_iter()
    .flatten()
    .collect();
    if !context.is_empty() {
        technical.push('\n');
        technical.push_str(&context.join(", "));
    }
    UserFacingError { title, causes, technical_details: technical, operation: operation.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptions() {
        let kinds = [
            (ErrorKind::Busy, "belegt"),
            (ErrorKind::NotFound, "nicht gefunden"),
            (ErrorKind::NetworkUnreachable, "Netzwerkadapter"),
            (ErrorKind::Configuration, "Verbindungsart"),
            (ErrorKind::Open, "nicht geöffnet"),
            (ErrorKind::HandshakeTimeout, "Sendefreigabe (CTS)"),
            (ErrorKind::Timeout, "Zeitüberschreitung"),
            (ErrorKind::ConnectionLost, "unterbrochen"),
            (ErrorKind::Write, "Schreibfehler"),
            (ErrorKind::Read, "Lesefehler"),
            (ErrorKind::Lsv2LinkTimeout, "DLE 0"),
            (ErrorKind::Lsv2AckTimeout, "nicht quittiert"),
            (ErrorKind::Lsv2RetryExceeded, "Wiederholungen"),
            (ErrorKind::CncAbort, "CNC hat die Übertragung beendet"),
            (ErrorKind::Lsv2Protocol, "LSV2-Protokollfehler"),
        ];
        for (kind, text) in kinds {
            let d = describe(&TransportError::new(kind, "x", "COM3"), "Senden", "p");
            assert!(d.title.contains(text), "{kind:?}: {}", d.title);
            assert!(!d.causes.is_empty());
            assert!(d.technical_details.contains("Vorgang: Senden, Port: COM3, Profil: p"));
        }
        let mut hs = TransportError::new(ErrorKind::HandshakeTimeout, "x", "");
        hs.missing = vec!["CTS".into(), "DSR".into()];
        assert!(describe(&hs, "", "").title.contains("CTS/DSR"));
        assert!(describe(&TransportError::new(ErrorKind::Open, "x", ""), "", "").title.starts_with("Der Port"));
    }
}
