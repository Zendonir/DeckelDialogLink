//! RS-232/Ethernet adapter via RFC 2217 (Telnet COM Port Control).
//!
//! DeckelLink negotiates baud rate, data format, flow control and control lines
//! with the adapter and receives the modem line states (CTS, DSR, RI, DCD).
//! Only an outgoing connection is opened; DeckelLink never listens on a port.

use std::io::{ErrorKind as IoKind, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use crate::clock::CancelToken;
use crate::model::{Parity, SerialSettings, StopBits};
use crate::transport::tcp::{validate_endpoint, CONNECT_TIMEOUT};
use crate::transport::{ErrorKind, ModemStatus, Transport, TransportError};

pub const IAC: u8 = 255;
pub const DONT: u8 = 254;
pub const DO: u8 = 253;
pub const WONT: u8 = 252;
pub const WILL: u8 = 251;
pub const SB: u8 = 250;
pub const SE: u8 = 240;
pub const BINARY: u8 = 0;
pub const SGA: u8 = 3;
pub const COM_PORT: u8 = 44;

pub const SET_BAUDRATE: u8 = 1;
pub const SET_DATASIZE: u8 = 2;
pub const SET_PARITY: u8 = 3;
pub const SET_STOPSIZE: u8 = 4;
pub const SET_CONTROL: u8 = 5;
pub const SET_MODEMSTATE_MASK: u8 = 11;
pub const PURGE_DATA: u8 = 12;
pub const SERVER_OFFSET: u8 = 100;
pub const NOTIFY_MODEMSTATE: u8 = 7;

/// How long the adapter may take to confirm the COM port option and the baud rate.
pub const NEGOTIATION_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParseState {
    Data,
    Iac,
    Option(u8),
    Sub,
    SubIac,
}

/// Incremental Telnet parser: separates serial data from Telnet/RFC 2217 commands.
#[derive(Debug, Clone)]
pub struct TelnetParser {
    state: ParseState,
    sub: Vec<u8>,
    /// Replies to send (refusals of unsupported options).
    pub replies: Vec<u8>,
    pub com_port_accepted: Option<bool>,
    pub baud_confirmed: bool,
    pub modem: ModemStatus,
}

impl Default for TelnetParser {
    fn default() -> Self {
        Self {
            state: ParseState::Data,
            sub: Vec::new(),
            replies: Vec::new(),
            com_port_accepted: None,
            baud_confirmed: false,
            modem: ModemStatus::default(),
        }
    }
}

impl TelnetParser {
    /// Feed raw socket bytes; returns the serial payload bytes.
    pub fn feed(&mut self, raw: &[u8]) -> Vec<u8> {
        let mut data = Vec::with_capacity(raw.len());
        for &byte in raw {
            match self.state {
                ParseState::Data => {
                    if byte == IAC {
                        self.state = ParseState::Iac;
                    } else {
                        data.push(byte);
                    }
                }
                ParseState::Iac => match byte {
                    IAC => {
                        data.push(IAC);
                        self.state = ParseState::Data;
                    }
                    DO | DONT | WILL | WONT => self.state = ParseState::Option(byte),
                    SB => {
                        self.sub.clear();
                        self.state = ParseState::Sub;
                    }
                    _ => self.state = ParseState::Data,
                },
                ParseState::Option(verb) => {
                    self.handle_option(verb, byte);
                    self.state = ParseState::Data;
                }
                ParseState::Sub => {
                    if byte == IAC {
                        self.state = ParseState::SubIac;
                    } else if self.sub.len() < 64 {
                        self.sub.push(byte);
                    }
                }
                ParseState::SubIac => {
                    if byte == SE {
                        self.handle_subnegotiation();
                        self.state = ParseState::Data;
                    } else {
                        if byte == IAC && self.sub.len() < 64 {
                            self.sub.push(IAC);
                        }
                        self.state = ParseState::Sub;
                    }
                }
            }
        }
        data
    }

    fn handle_option(&mut self, verb: u8, option: u8) {
        match (verb, option) {
            (DO, COM_PORT) => self.com_port_accepted = Some(true),
            (DONT, COM_PORT) => self.com_port_accepted = Some(false),
            (DO | WILL, BINARY | SGA) | (WONT | DONT, _) | (WILL, COM_PORT) => {}
            (DO, other) => self.replies.extend_from_slice(&[IAC, WONT, other]),
            (WILL, other) => self.replies.extend_from_slice(&[IAC, DONT, other]),
            _ => {}
        }
    }

    fn handle_subnegotiation(&mut self) {
        let mut parts = self.sub.iter().copied();
        if parts.next() != Some(COM_PORT) {
            return;
        }
        match parts.next() {
            Some(code) if code == SERVER_OFFSET + SET_BAUDRATE => self.baud_confirmed = true,
            Some(code) if code == SERVER_OFFSET + NOTIFY_MODEMSTATE => {
                if let Some(state) = parts.next() {
                    self.modem.cts = Some(state & 0x10 != 0);
                    self.modem.dsr = Some(state & 0x20 != 0);
                    self.modem.ri = Some(state & 0x40 != 0);
                    self.modem.dcd = Some(state & 0x80 != 0);
                }
            }
            _ => {}
        }
    }
}

/// Escape IAC bytes in serial payload data.
pub fn escape(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 4);
    for &b in data {
        out.push(b);
        if b == IAC {
            out.push(IAC);
        }
    }
    out
}

fn subnegotiation(command: u8, value: &[u8]) -> Vec<u8> {
    let mut out = vec![IAC, SB, COM_PORT, command];
    out.extend(escape(value));
    out.extend_from_slice(&[IAC, SE]);
    out
}

/// Negotiation sequence for the given serial settings.
pub fn negotiation(settings: &SerialSettings) -> Vec<u8> {
    let mut out = vec![IAC, WILL, COM_PORT, IAC, WILL, BINARY, IAC, DO, BINARY, IAC, WILL, SGA, IAC, DO, SGA];
    out.extend(subnegotiation(SET_BAUDRATE, &settings.baud_rate.to_be_bytes()));
    out.extend(subnegotiation(SET_DATASIZE, &[settings.data_bits]));
    let parity = match settings.parity {
        Parity::None => 1,
        Parity::Odd => 2,
        Parity::Even => 3,
        Parity::Mark => 4,
        Parity::Space => 5,
    };
    out.extend(subnegotiation(SET_PARITY, &[parity]));
    let stop = match settings.stop_bits {
        StopBits::One => 1,
        StopBits::Two => 2,
        StopBits::OnePointFive => 3,
    };
    out.extend(subnegotiation(SET_STOPSIZE, &[stop]));
    let flow = if settings.rts_cts {
        3
    } else if settings.xon_xoff {
        2
    } else {
        1
    };
    out.extend(subnegotiation(SET_CONTROL, &[flow]));
    out.extend(subnegotiation(SET_CONTROL, &[if settings.initial_dtr { 8 } else { 9 }]));
    if !settings.rts_cts {
        out.extend(subnegotiation(SET_CONTROL, &[if settings.initial_rts { 11 } else { 12 }]));
    }
    out.extend(subnegotiation(SET_MODEMSTATE_MASK, &[0xF0]));
    out
}

#[derive(Debug)]
pub struct Rfc2217Transport {
    host: String,
    port: u32,
    settings: SerialSettings,
    stream: Option<TcpStream>,
    parser: TelnetParser,
    pending: Vec<u8>,
    rts: bool,
    dtr: bool,
}

impl Rfc2217Transport {
    pub fn new(host: impl Into<String>, port: u32, settings: SerialSettings) -> Self {
        let (rts, dtr) = (settings.initial_rts, settings.initial_dtr);
        Self {
            host: host.into().trim().to_string(),
            port,
            settings,
            stream: None,
            parser: TelnetParser::default(),
            pending: Vec::new(),
            rts,
            dtr,
        }
    }

    fn label(&self) -> String {
        format!("RFC2217 {}:{}", self.host, self.port)
    }

    fn error(&self, kind: ErrorKind, message: impl Into<String>) -> TransportError {
        TransportError::new(kind, message, self.label())
    }

    fn addresses(&self) -> Result<Vec<SocketAddr>, TransportError> {
        let host = self.host.trim_start_matches('[').trim_end_matches(']');
        let port = u16::try_from(self.port).map_err(|_| self.error(ErrorKind::Configuration, "Ungültiger TCP-Port"))?;
        (host, port).to_socket_addrs().map(Iterator::collect).map_err(|e| {
            self.error(ErrorKind::NetworkUnreachable, format!("{} ist nicht erreichbar: {e}", self.label()))
        })
    }

    fn send_raw(&mut self, raw: &[u8]) -> Result<(), TransportError> {
        let label = self.label();
        let Some(stream) = self.stream.as_mut() else {
            return Err(TransportError::new(ErrorKind::ConnectionLost, format!("{label} ist nicht verbunden."), label));
        };
        match stream.write_all(raw) {
            Ok(()) => Ok(()),
            Err(e) if matches!(e.kind(), IoKind::TimedOut | IoKind::WouldBlock) => {
                Err(TransportError::new(ErrorKind::Timeout, format!("Schreib-Timeout: {e}"), label))
            }
            Err(e) => {
                self.stream = None;
                Err(TransportError::new(ErrorKind::ConnectionLost, format!("Schreibfehler: {e}"), label))
            }
        }
    }

    /// Read from the socket once; payload goes to `pending`. Returns false on timeout.
    fn pump(&mut self) -> Result<bool, TransportError> {
        let label = self.label();
        let Some(stream) = self.stream.as_mut() else {
            return Err(TransportError::new(ErrorKind::ConnectionLost, format!("{label} ist nicht verbunden."), label));
        };
        let mut buffer = [0u8; 512];
        match stream.read(&mut buffer) {
            Ok(0) => {
                self.stream = None;
                Err(TransportError::new(ErrorKind::ConnectionLost, "Verbindung vom Adapter geschlossen", label))
            }
            Ok(n) => {
                let data = self.parser.feed(buffer.get(..n).unwrap_or_default());
                self.pending.extend(data);
                if !self.parser.replies.is_empty() {
                    let replies = std::mem::take(&mut self.parser.replies);
                    self.send_raw(&replies)?;
                }
                Ok(true)
            }
            Err(e) if matches!(e.kind(), IoKind::TimedOut | IoKind::WouldBlock) => Ok(false),
            Err(e) => {
                self.stream = None;
                Err(TransportError::new(ErrorKind::ConnectionLost, format!("Lesefehler: {e}"), label))
            }
        }
    }
}

impl Transport for Rfc2217Transport {
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
        if self.settings.dsr_dtr {
            return Err(
                self.error(ErrorKind::Configuration, "DSR/DTR-Flusssteuerung ist über RFC 2217 nicht einstellbar.")
            );
        }
        let mut connected = None;
        let mut last_error = String::new();
        for address in self.addresses()? {
            match TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) {
                Ok(stream) => {
                    connected = Some(stream);
                    break;
                }
                Err(e) => last_error = e.to_string(),
            }
        }
        let Some(stream) = connected else {
            return Err(self
                .error(ErrorKind::NetworkUnreachable, format!("{} ist nicht erreichbar: {last_error}", self.label())));
        };
        let read = Duration::from_millis(self.settings.read_timeout_ms.clamp(1, 200));
        let write = Duration::from_millis(self.settings.write_timeout_ms.max(1));
        stream
            .set_read_timeout(Some(read))
            .and_then(|()| stream.set_write_timeout(Some(write)))
            .and_then(|()| stream.set_nodelay(true))
            .map_err(|e| self.error(ErrorKind::Open, format!("Verbindung nicht einstellbar: {e}")))?;
        self.stream = Some(stream);
        self.parser = TelnetParser::default();
        self.pending.clear();
        let negotiation = negotiation(&self.settings);
        self.send_raw(&negotiation)?;
        let deadline = Instant::now() + NEGOTIATION_TIMEOUT;
        while Instant::now() < deadline && !(self.parser.com_port_accepted.is_some() && self.parser.baud_confirmed) {
            if self.parser.com_port_accepted == Some(false) {
                break;
            }
            self.pump()?;
        }
        if self.parser.com_port_accepted != Some(true) {
            self.close();
            return Err(self.error(
                ErrorKind::Configuration,
                "Der Adapter unterstützt RFC 2217 nicht (keine Zusage für COM-Port-Steuerung). Adapter auf RFC 2217 \
                 stellen oder Verbindungsart 'TCP-Rohdaten' wählen.",
            ));
        }
        Ok(())
    }

    fn close(&mut self) {
        if let Some(stream) = self.stream.take() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    }

    fn write(&mut self, data: &[u8], cancel: &CancelToken) -> Result<usize, TransportError> {
        if cancel.is_cancelled() || data.is_empty() {
            return Ok(0);
        }
        self.send_raw(&escape(data))?;
        Ok(data.len())
    }

    fn read(&mut self, max: usize, cancel: &CancelToken) -> Result<Vec<u8>, TransportError> {
        let deadline = Instant::now() + Duration::from_millis(self.settings.read_timeout_ms.max(1));
        loop {
            if cancel.is_cancelled() {
                return Ok(Vec::new());
            }
            if !self.pending.is_empty() {
                let count = max.max(1).min(self.pending.len());
                return Ok(self.pending.drain(..count).collect());
            }
            if Instant::now() >= deadline {
                return Ok(Vec::new());
            }
            self.pump()?;
        }
    }

    fn out_waiting(&mut self) -> usize {
        0
    }

    fn modem_status(&mut self) -> ModemStatus {
        if self.stream.is_none() {
            return ModemStatus::default();
        }
        ModemStatus { rts: Some(self.rts), dtr: Some(self.dtr), ..self.parser.modem }
    }

    fn set_control_lines(&mut self, rts: Option<bool>, dtr: Option<bool>) -> Result<(), TransportError> {
        if let Some(rts) = rts {
            self.send_raw(&subnegotiation(SET_CONTROL, &[if rts { 11 } else { 12 }]))?;
            self.rts = rts;
        }
        if let Some(dtr) = dtr {
            self.send_raw(&subnegotiation(SET_CONTROL, &[if dtr { 8 } else { 9 }]))?;
            self.dtr = dtr;
        }
        Ok(())
    }

    fn reset_input(&mut self) {
        self.pending.clear();
        let _ = self.send_raw(&subnegotiation(PURGE_DATA, &[1]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn parser_separates_data_and_commands() {
        let mut p = TelnetParser::default();
        let raw = [
            b'A', IAC, IAC, b'B', IAC, DO, COM_PORT, IAC, DO, 24, IAC, WILL, 31, IAC, SB, COM_PORT, 101, 0, 0, 0x12,
            0xC0, IAC, SE, IAC, SB, COM_PORT, 107, 0x30, IAC, SE, IAC, 241, b'C',
        ];
        assert_eq!(p.feed(&raw), vec![b'A', IAC, b'B', b'C']);
        assert_eq!(p.com_port_accepted, Some(true));
        assert!(p.baud_confirmed);
        assert_eq!(
            (p.modem.cts, p.modem.dsr, p.modem.ri, p.modem.dcd),
            (Some(true), Some(true), Some(false), Some(false))
        );
        assert_eq!(p.replies, vec![IAC, WONT, 24, IAC, DONT, 31]);
        let mut refuse = TelnetParser::default();
        refuse.feed(&[IAC, DONT, COM_PORT]);
        assert_eq!(refuse.com_port_accepted, Some(false));
        assert_eq!(escape(&[1, IAC, 2]), vec![1, IAC, IAC, 2]);
    }

    #[test]
    fn negotiation_contains_settings() {
        let s = SerialSettings { parity: Parity::Even, rts_cts: true, ..SerialSettings::default() };
        let n = negotiation(&s);
        let has = |seq: &[u8]| n.windows(seq.len()).any(|w| w == seq);
        assert!(has(&[IAC, WILL, COM_PORT]));
        assert!(has(&[IAC, SB, COM_PORT, SET_BAUDRATE, 0, 0, 0x12, 0xC0, IAC, SE]));
        assert!(has(&[COM_PORT, SET_DATASIZE, 7]));
        assert!(has(&[COM_PORT, SET_PARITY, 3]));
        assert!(has(&[COM_PORT, SET_CONTROL, 3]));
        assert!(!has(&[COM_PORT, SET_CONTROL, 11]));
    }

    /// Minimal RFC 2217 test adapter on localhost (test only): accepts the option, confirms the
    /// baud rate, reports CTS/DSR and echoes payload bytes.
    fn fake_adapter(accept: bool) -> (u32, std::thread::JoinHandle<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = u32::from(listener.local_addr().unwrap().port());
        let handle = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket.set_read_timeout(Some(Duration::from_millis(500))).unwrap();
            let answer: Vec<u8> = if accept {
                vec![
                    IAC, DO, COM_PORT, IAC, SB, COM_PORT, 101, 0, 0, 0x12, 0xC0, IAC, SE, IAC, SB, COM_PORT, 107, 0x30,
                    IAC, SE,
                ]
            } else {
                vec![IAC, DONT, COM_PORT]
            };
            socket.write_all(&answer).unwrap();
            let mut parser = TelnetParser::default();
            let mut payload = Vec::new();
            let mut buffer = [0u8; 256];
            loop {
                match socket.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let data = parser.feed(&buffer[..n]);
                        if !data.is_empty() {
                            socket.write_all(&escape(&data)).unwrap();
                            payload.extend(data);
                        }
                    }
                }
            }
            payload
        });
        (port, handle)
    }

    #[test]
    fn roundtrip_with_test_adapter() {
        let (port, server) = fake_adapter(true);
        let settings = SerialSettings { read_timeout_ms: 1000, ..SerialSettings::default() };
        let mut t = Rfc2217Transport::new("127.0.0.1", port, settings);
        let token = CancelToken::new();
        t.open().unwrap();
        assert!(t.is_open() && !t.is_simulated());
        assert_eq!(t.modem_status().cts, Some(true));
        let data = [b'N', b'1', IAC, b'\r', b'\n'];
        assert_eq!(t.write(&data, &token).unwrap(), 5);
        let mut got = Vec::new();
        while got.len() < 5 {
            got.extend(t.read(16, &token).unwrap());
        }
        assert_eq!(got, data);
        t.set_control_lines(Some(false), Some(true)).unwrap();
        assert_eq!(t.modem_status().rts, Some(false));
        t.reset_input();
        t.close();
        assert_eq!(server.join().unwrap(), data);
        assert_eq!(t.modem_status(), ModemStatus::default());
    }

    #[test]
    fn adapter_without_rfc2217_is_reported() {
        let (port, server) = fake_adapter(false);
        let mut t = Rfc2217Transport::new("127.0.0.1", port, SerialSettings::default());
        let err = t.open().unwrap_err();
        assert_eq!(err.kind, ErrorKind::Configuration);
        assert!(err.message.contains("RFC 2217"));
        let _ = server.join();
        let bad = SerialSettings { dsr_dtr: true, ..SerialSettings::default() };
        assert_eq!(Rfc2217Transport::new("127.0.0.1", port, bad).open().unwrap_err().kind, ErrorKind::Configuration);
        assert_eq!(
            Rfc2217Transport::new("", 4001, SerialSettings::default()).open().unwrap_err().kind,
            ErrorKind::Configuration
        );
        let mut unreachable = Rfc2217Transport::new("127.0.0.1", 1, SerialSettings::default());
        assert_eq!(unreachable.open().unwrap_err().kind, ErrorKind::NetworkUnreachable);
        let token = CancelToken::new();
        assert!(unreachable.read(1, &token).is_err());
        assert_eq!(unreachable.out_waiting(), 0);
    }
}
