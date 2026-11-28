//! Real RS-232 transport based on the `serialport` crate.

use std::io::{ErrorKind as IoKind, Read, Write};
use std::time::Duration;

use crate::clock::CancelToken;
use crate::model::{Parity, SerialSettings, StopBits};
use crate::transport::{ErrorKind, ModemStatus, Transport, TransportError};

/// A serial port known to the operating system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortInfo {
    pub device: String,
    pub description: String,
}

impl PortInfo {
    pub fn label(&self) -> String {
        if self.description.is_empty() || self.description == self.device {
            self.device.clone()
        } else {
            format!("{} - {}", self.device, self.description)
        }
    }
}

/// List serial ports known to the OS. Never fails (returns an empty list instead).
pub fn available_ports() -> Vec<PortInfo> {
    let mut ports: Vec<PortInfo> = serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .map(|p| {
            let description = match p.port_type {
                serialport::SerialPortType::UsbPort(usb) => {
                    usb.product.or(usb.manufacturer).unwrap_or_else(|| "USB".into())
                }
                serialport::SerialPortType::BluetoothPort => "Bluetooth".into(),
                serialport::SerialPortType::PciPort => "PCI".into(),
                serialport::SerialPortType::Unknown => String::new(),
            };
            PortInfo { device: p.port_name, description }
        })
        .collect();
    ports.sort_by(|a, b| a.device.cmp(&b.device));
    ports
}

/// Settings the `serialport` crate cannot represent are reported instead of silently changed.
pub fn unsupported_settings(s: &SerialSettings) -> Vec<String> {
    let mut problems = Vec::new();
    if s.stop_bits == StopBits::OnePointFive {
        problems.push("1,5 Stoppbits werden von diesem Treiber nicht unterstützt.".to_string());
    }
    if matches!(s.parity, Parity::Mark | Parity::Space) {
        problems.push(format!("Parität '{}' wird von diesem Treiber nicht unterstützt.", s.parity.label()));
    }
    if s.dsr_dtr {
        problems.push("DSR/DTR-Flusssteuerung wird von diesem Treiber nicht unterstützt.".to_string());
    }
    if [s.rts_cts, s.xon_xoff].iter().all(|b| *b) {
        problems.push("RTS/CTS und XON/XOFF gleichzeitig werden von diesem Treiber nicht unterstützt.".to_string());
    }
    problems
}

pub(crate) fn classify_open_error(err: &serialport::Error, port: &str) -> TransportError {
    let text = err.to_string().to_lowercase();
    let busy = ["access is denied", "zugriff verweigert", "busy", "in use", "permission denied"];
    let missing = ["no such file", "cannot find", "could not find", "nicht finden", "not found"];
    let kind = match err.kind() {
        serialport::ErrorKind::NoDevice => ErrorKind::NotFound,
        serialport::ErrorKind::Io(IoKind::NotFound) => ErrorKind::NotFound,
        serialport::ErrorKind::Io(IoKind::PermissionDenied) => ErrorKind::Busy,
        _ if busy.iter().any(|m| text.contains(m)) => ErrorKind::Busy,
        _ if missing.iter().any(|m| text.contains(m)) => ErrorKind::NotFound,
        _ => ErrorKind::Open,
    };
    let message = match kind {
        ErrorKind::Busy => format!("{port} ist belegt oder der Zugriff wurde verweigert: {err}"),
        ErrorKind::NotFound => format!("{port} wurde nicht gefunden: {err}"),
        _ => format!("{port} konnte nicht geöffnet werden: {err}"),
    };
    TransportError::new(kind, message, port)
}

/// Real serial port.
pub struct SerialTransport {
    port_name: String,
    settings: SerialSettings,
    port: Option<Box<dyn serialport::SerialPort>>,
}

impl std::fmt::Debug for SerialTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SerialTransport").field("port", &self.port_name).field("open", &self.port.is_some()).finish()
    }
}

impl SerialTransport {
    pub fn new(port_name: impl Into<String>, settings: SerialSettings) -> Self {
        Self { port_name: port_name.into(), settings, port: None }
    }

    fn require_open(&mut self) -> Result<&mut Box<dyn serialport::SerialPort>, TransportError> {
        let name = self.port_name.clone();
        self.port
            .as_mut()
            .ok_or_else(|| TransportError::new(ErrorKind::ConnectionLost, format!("{name} ist nicht geöffnet."), name))
    }
}

impl Transport for SerialTransport {
    fn name(&self) -> String {
        self.port_name.clone()
    }

    fn is_open(&self) -> bool {
        self.port.is_some()
    }

    fn is_simulated(&self) -> bool {
        false
    }

    fn open(&mut self) -> Result<(), TransportError> {
        if self.port.is_some() {
            return Ok(());
        }
        if self.port_name.is_empty() {
            return Err(TransportError::new(ErrorKind::Open, "Kein COM-Port ausgewählt.", ""));
        }
        let problems = unsupported_settings(&self.settings);
        if !problems.is_empty() {
            return Err(TransportError::new(ErrorKind::Configuration, problems.join(" "), self.port_name.clone()));
        }
        let s = &self.settings;
        let data_bits = match s.data_bits {
            5 => serialport::DataBits::Five,
            6 => serialport::DataBits::Six,
            7 => serialport::DataBits::Seven,
            _ => serialport::DataBits::Eight,
        };
        let parity = match s.parity {
            Parity::Even => serialport::Parity::Even,
            Parity::Odd => serialport::Parity::Odd,
            _ => serialport::Parity::None,
        };
        let stop_bits =
            if s.stop_bits == StopBits::Two { serialport::StopBits::Two } else { serialport::StopBits::One };
        let flow = if s.rts_cts {
            serialport::FlowControl::Hardware
        } else if s.xon_xoff {
            serialport::FlowControl::Software
        } else {
            serialport::FlowControl::None
        };
        let mut port = serialport::new(&self.port_name, s.baud_rate)
            .data_bits(data_bits)
            .parity(parity)
            .stop_bits(stop_bits)
            .flow_control(flow)
            .timeout(Duration::from_millis(s.read_timeout_ms.max(1)))
            .open()
            .map_err(|e| classify_open_error(&e, &self.port_name))?;
        if !s.rts_cts {
            // With hardware handshake the driver controls RTS.
            let _ = port.write_request_to_send(s.initial_rts);
        }
        let _ = port.write_data_terminal_ready(s.initial_dtr);
        self.port = Some(port);
        Ok(())
    }

    fn close(&mut self) {
        // Dropping the handle closes the port; never panics.
        self.port = None;
    }

    fn write(&mut self, data: &[u8], cancel: &CancelToken) -> Result<usize, TransportError> {
        if cancel.is_cancelled() {
            return Ok(0);
        }
        let name = self.port_name.clone();
        let port = self.require_open()?;
        match port.write(data) {
            Ok(n) => Ok(n),
            Err(e) if e.kind() == IoKind::TimedOut => Ok(0),
            Err(e) if matches!(e.kind(), IoKind::BrokenPipe | IoKind::NotConnected | IoKind::UnexpectedEof) => {
                Err(TransportError::new(ErrorKind::ConnectionLost, format!("Schreibfehler auf {name}: {e}"), name))
            }
            Err(e) => Err(TransportError::new(ErrorKind::Write, format!("Schreibfehler auf {name}: {e}"), name)),
        }
    }

    fn read(&mut self, max: usize, cancel: &CancelToken) -> Result<Vec<u8>, TransportError> {
        if cancel.is_cancelled() {
            return Ok(Vec::new());
        }
        let name = self.port_name.clone();
        let port = self.require_open()?;
        let mut buffer = vec![0u8; max.max(1)];
        match port.read(&mut buffer) {
            Ok(n) => {
                buffer.truncate(n);
                Ok(buffer)
            }
            Err(e) if e.kind() == IoKind::TimedOut || e.kind() == IoKind::WouldBlock => Ok(Vec::new()),
            Err(e) if matches!(e.kind(), IoKind::BrokenPipe | IoKind::NotConnected | IoKind::UnexpectedEof) => {
                Err(TransportError::new(ErrorKind::ConnectionLost, format!("Lesefehler auf {name}: {e}"), name))
            }
            Err(e) => Err(TransportError::new(ErrorKind::Read, format!("Lesefehler auf {name}: {e}"), name)),
        }
    }

    fn out_waiting(&mut self) -> usize {
        self.port.as_ref().and_then(|p| p.bytes_to_write().ok()).map_or(0, |n| usize::try_from(n).unwrap_or(0))
    }

    fn modem_status(&mut self) -> ModemStatus {
        let rts = self.settings.initial_rts;
        let dtr = self.settings.initial_dtr;
        let Some(port) = self.port.as_mut() else {
            return ModemStatus::default();
        };
        ModemStatus {
            cts: port.read_clear_to_send().ok(),
            dsr: port.read_data_set_ready().ok(),
            dcd: port.read_carrier_detect().ok(),
            ri: port.read_ring_indicator().ok(),
            rts: Some(rts),
            dtr: Some(dtr),
        }
    }

    fn set_control_lines(&mut self, rts: Option<bool>, dtr: Option<bool>) -> Result<(), TransportError> {
        let name = self.port_name.clone();
        let port = self.require_open()?;
        if let Some(rts) = rts {
            port.write_request_to_send(rts).map_err(|e| {
                TransportError::new(ErrorKind::Write, format!("RTS setzen fehlgeschlagen: {e}"), name.clone())
            })?;
            self.settings.initial_rts = rts;
        }
        if let Some(dtr) = dtr {
            let port = self.require_open()?;
            port.write_data_terminal_ready(dtr)
                .map_err(|e| TransportError::new(ErrorKind::Write, format!("DTR setzen fehlgeschlagen: {e}"), name))?;
            self.settings.initial_dtr = dtr;
        }
        Ok(())
    }

    fn reset_input(&mut self) {
        if let Some(port) = self.port.as_mut() {
            let _ = port.clear(serialport::ClearBuffer::Input);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_settings_are_reported() {
        assert!(unsupported_settings(&SerialSettings::default()).is_empty());
        let s = SerialSettings {
            stop_bits: StopBits::OnePointFive,
            parity: Parity::Mark,
            dsr_dtr: true,
            rts_cts: true,
            xon_xoff: true,
            ..SerialSettings::default()
        };
        assert_eq!(unsupported_settings(&s).len(), 4);
    }

    #[test]
    fn opening_missing_port_fails_cleanly() {
        let mut t = SerialTransport::new("", SerialSettings::default());
        assert_eq!(t.open().unwrap_err().kind, ErrorKind::Open);
        let mut t = SerialTransport::new("/dev/deckellink-gibt-es-nicht", SerialSettings::default());
        let err = t.open().unwrap_err();
        assert!(matches!(err.kind, ErrorKind::NotFound | ErrorKind::Open), "{err:?}");
        assert!(!t.is_open() && !t.is_simulated());
        let token = CancelToken::new();
        assert_eq!(t.write(b"x", &token).unwrap_err().kind, ErrorKind::ConnectionLost);
        assert_eq!(t.read(1, &token).unwrap_err().kind, ErrorKind::ConnectionLost);
        assert_eq!(t.out_waiting(), 0);
        assert_eq!(t.modem_status(), ModemStatus::default());
        t.reset_input();
        t.close();
        let bad = SerialSettings { dsr_dtr: true, ..SerialSettings::default() };
        assert_eq!(SerialTransport::new("COM1", bad).open().unwrap_err().kind, ErrorKind::Configuration);
        let _ = available_ports();
        assert_eq!(PortInfo { device: "COM3".into(), description: "USB".into() }.label(), "COM3 - USB");
        assert_eq!(PortInfo { device: "COM3".into(), description: String::new() }.label(), "COM3");
    }
}
