//! RS-232/Ethernet adapter ("serial device server") as raw TCP connection.
//!
//! In raw TCP mode the serial parameters (baud rate, parity, handshake) are set
//! **in the adapter**; modem lines are not visible to DeckelLink. RFC 2217 follows
//! in a later stage of the Rust port. DeckelLink only connects outwards and never
//! opens a listening port.

use std::io::{ErrorKind as IoKind, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::clock::CancelToken;
use crate::model::SerialSettings;
use crate::transport::{ErrorKind, ModemStatus, Transport, TransportError};

pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// German error text, or `None` if host/port are usable.
pub fn validate_endpoint(host: &str, port: u32) -> Option<String> {
    let host = host.trim();
    if host.is_empty() {
        return Some("Keine IP-Adresse bzw. kein Hostname des Netzwerkadapters angegeben".into());
    }
    let valid = host.len() <= 253 && host.chars().all(|c| c.is_ascii_alphanumeric() || ".-:[]".contains(c));
    if !valid {
        return Some(format!("Ungültige Adresse: '{host}'"));
    }
    if !(1..=65_535).contains(&port) {
        return Some(format!("Ungültiger TCP-Port: {port}"));
    }
    None
}

#[derive(Debug)]
pub struct TcpTransport {
    host: String,
    port: u32,
    settings: SerialSettings,
    stream: Option<TcpStream>,
}

impl TcpTransport {
    pub fn new(host: impl Into<String>, port: u32, settings: SerialSettings) -> Self {
        Self { host: host.into().trim().to_string(), port, settings, stream: None }
    }

    fn label(&self) -> String {
        format!("TCP {}:{}", self.host, self.port)
    }

    fn error(&self, kind: ErrorKind, message: String) -> TransportError {
        TransportError::new(kind, message, self.label())
    }

    fn addresses(&self) -> Result<Vec<SocketAddr>, TransportError> {
        let host = self.host.trim_start_matches('[').trim_end_matches(']');
        let port =
            u16::try_from(self.port).map_err(|_| self.error(ErrorKind::Configuration, "Ungültiger TCP-Port".into()))?;
        (host, port).to_socket_addrs().map(Iterator::collect).map_err(|e| {
            self.error(ErrorKind::NetworkUnreachable, format!("{} ist nicht erreichbar: {e}", self.label()))
        })
    }
}

impl Transport for TcpTransport {
    fn name(&self) -> String {
        self.label()
    }

    fn is_open(&self) -> bool {
        self.stream.is_some()
    }

    fn is_simulated(&self) -> bool {
        false
    }

    fn open(&mut self) -> Result<(), TransportError> {
        if self.stream.is_some() {
            return Ok(());
        }
        if let Some(problem) = validate_endpoint(&self.host, self.port) {
            return Err(self.error(ErrorKind::Configuration, problem));
        }
        if self.settings.require_cts_before_send || self.settings.require_dsr_before_send {
            return Err(self.error(
                ErrorKind::Configuration,
                "Bei einer TCP-Rohverbindung sind CTS/DSR für DeckelLink nicht sichtbar. Im Profil 'CTS/DSR vor dem \
                 Senden erforderlich' abschalten und den Handshake im Adapter konfigurieren."
                    .into(),
            ));
        }
        let mut last_error = None;
        for address in self.addresses()? {
            match TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) {
                Ok(stream) => {
                    let read = Duration::from_millis(self.settings.read_timeout_ms.max(1));
                    let write = Duration::from_millis(self.settings.write_timeout_ms.max(1));
                    let configured = stream
                        .set_read_timeout(Some(read))
                        .and_then(|()| stream.set_write_timeout(Some(write)))
                        .and_then(|()| stream.set_nodelay(true));
                    if let Err(e) = configured {
                        return Err(self.error(ErrorKind::Open, format!("Verbindung nicht einstellbar: {e}")));
                    }
                    self.stream = Some(stream);
                    return Ok(());
                }
                Err(e) => last_error = Some(e),
            }
        }
        let detail = last_error.map_or_else(|| "keine Adresse".to_string(), |e| e.to_string());
        Err(self.error(ErrorKind::NetworkUnreachable, format!("{} ist nicht erreichbar: {detail}", self.label())))
    }

    fn close(&mut self) {
        if let Some(stream) = self.stream.take() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    }

    fn write(&mut self, data: &[u8], cancel: &CancelToken) -> Result<usize, TransportError> {
        if cancel.is_cancelled() {
            return Ok(0);
        }
        let label = self.label();
        let Some(stream) = self.stream.as_mut() else {
            return Err(TransportError::new(ErrorKind::ConnectionLost, format!("{label} ist nicht verbunden."), label));
        };
        match stream.write(data) {
            Ok(0) if !data.is_empty() => {
                self.stream = None;
                Err(TransportError::new(ErrorKind::ConnectionLost, "Verbindung vom Adapter geschlossen", label))
            }
            Ok(n) => Ok(n),
            Err(e) if matches!(e.kind(), IoKind::TimedOut | IoKind::WouldBlock) => Ok(0),
            Err(e) => {
                self.stream = None;
                Err(TransportError::new(ErrorKind::ConnectionLost, format!("Schreibfehler: {e}"), label))
            }
        }
    }

    fn read(&mut self, max: usize, cancel: &CancelToken) -> Result<Vec<u8>, TransportError> {
        if cancel.is_cancelled() {
            return Ok(Vec::new());
        }
        let label = self.label();
        let Some(stream) = self.stream.as_mut() else {
            return Err(TransportError::new(ErrorKind::ConnectionLost, format!("{label} ist nicht verbunden."), label));
        };
        let mut buffer = vec![0u8; max.max(1)];
        match stream.read(&mut buffer) {
            Ok(0) => {
                self.stream = None;
                Err(TransportError::new(ErrorKind::ConnectionLost, "Verbindung vom Adapter geschlossen", label))
            }
            Ok(n) => {
                buffer.truncate(n);
                Ok(buffer)
            }
            Err(e) if matches!(e.kind(), IoKind::TimedOut | IoKind::WouldBlock) => Ok(Vec::new()),
            Err(e) => {
                self.stream = None;
                Err(TransportError::new(ErrorKind::ConnectionLost, format!("Lesefehler: {e}"), label))
            }
        }
    }

    fn out_waiting(&mut self) -> usize {
        0
    }

    fn modem_status(&mut self) -> ModemStatus {
        ModemStatus::default()
    }

    fn set_control_lines(&mut self, _rts: Option<bool>, _dtr: Option<bool>) -> Result<(), TransportError> {
        Err(self.error(
            ErrorKind::Configuration,
            "Steuerleitungen sind bei einer TCP-Rohverbindung nicht schaltbar.".into(),
        ))
    }

    fn reset_input(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn endpoint_validation() {
        assert!(validate_endpoint("", 4001).is_some());
        assert!(validate_endpoint("a b", 4001).is_some());
        assert!(validate_endpoint("192.168.0.10", 0).is_some());
        assert!(validate_endpoint("192.168.0.10", 4001).is_none());
        assert!(validate_endpoint("[::1]", 4001).is_none());
    }

    #[test]
    fn roundtrip_against_local_test_server() {
        // A local loopback listener stands in for the adapter (test only).
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = u32::from(listener.local_addr().unwrap().port());
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut buf = [0u8; 5];
            socket.read_exact(&mut buf).unwrap();
            socket.write_all(&buf).unwrap();
        });
        let settings = SerialSettings { read_timeout_ms: 2000, ..SerialSettings::default() };
        let mut t = TcpTransport::new("127.0.0.1", port, settings);
        let token = CancelToken::new();
        t.open().unwrap();
        assert!(t.is_open() && !t.is_simulated());
        assert_eq!(t.write(b"HALLO", &token).unwrap(), 5);
        let mut got = Vec::new();
        while got.len() < 5 {
            got.extend(t.read(16, &token).unwrap());
        }
        assert_eq!(got, b"HALLO");
        server.join().unwrap();
        assert_eq!(t.read(16, &token).unwrap_err().kind, ErrorKind::ConnectionLost);
        assert_eq!(t.modem_status(), ModemStatus::default());
        assert!(t.set_control_lines(Some(true), None).is_err());
        t.close();
    }

    #[test]
    fn open_errors() {
        let cts = SerialSettings { require_cts_before_send: true, ..SerialSettings::default() };
        assert_eq!(TcpTransport::new("127.0.0.1", 4001, cts).open().unwrap_err().kind, ErrorKind::Configuration);
        let mut t = TcpTransport::new("", 4001, SerialSettings::default());
        assert_eq!(t.open().unwrap_err().kind, ErrorKind::Configuration);
        // nothing listens on port 1 of localhost
        let mut t = TcpTransport::new("127.0.0.1", 1, SerialSettings::default());
        assert_eq!(t.open().unwrap_err().kind, ErrorKind::NetworkUnreachable);
        let token = CancelToken::new();
        assert!(t.write(b"x", &token).is_err() && t.read(1, &token).is_err());
        t.reset_input();
        assert_eq!(t.out_waiting(), 0);
    }
}
