//! Simulated CNC endpoint (replaces only the transport layer).
//!
//! Timing follows the profile's baud rate and the injected [`Clock`], so tests are
//! deterministic with [`crate::clock::FakeClock`].

use std::sync::{Arc, Mutex, PoisonError};

use crate::clock::{CancelToken, Clock};
use crate::model::SerialSettings;
use crate::transport::{ErrorKind, ModemStatus, Transport, TransportError};

pub const SIMULATED_PORT_NAME: &str = "SIMULATOR";

/// Behaviour of the simulated machine/connection.
#[derive(Debug, Clone, PartialEq)]
pub struct SimulationConfig {
    pub speed_factor: f64,
    /// "busy" | "not_found" | "generic"
    pub open_error: Option<String>,
    pub cts: bool,
    pub dsr: bool,
    pub dcd: bool,
    pub ri: bool,
    pub cts_on_after_s: Option<f64>,
    pub max_write_chunk: usize,
    pub write_stall_after_bytes: Option<usize>,
    pub write_error_after_bytes: Option<usize>,
    pub disconnect_after_bytes: Option<usize>,
    pub machine_program: Vec<u8>,
    pub machine_send_delay_s: f64,
    pub read_chunk_sizes: Vec<usize>,
    pub corrupt_offsets: Vec<usize>,
    pub inject_null_every: usize,
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            speed_factor: 1.0,
            open_error: None,
            cts: true,
            dsr: true,
            dcd: false,
            ri: false,
            cts_on_after_s: None,
            max_write_chunk: 0,
            write_stall_after_bytes: None,
            write_error_after_bytes: None,
            disconnect_after_bytes: None,
            machine_program: Vec::new(),
            machine_send_delay_s: 0.5,
            read_chunk_sizes: Vec::new(),
            corrupt_offsets: Vec::new(),
            inject_null_every: 0,
        }
    }
}

/// Apply null injection and corruption to the machine program.
pub fn build_machine_stream(config: &SimulationConfig) -> Vec<u8> {
    let mut data = Vec::with_capacity(config.machine_program.len());
    for (index, byte) in config.machine_program.iter().enumerate() {
        data.push(*byte);
        if config.inject_null_every > 0 && (index + 1) % config.inject_null_every == 0 {
            data.push(0);
        }
    }
    for offset in &config.corrupt_offsets {
        if let Some(byte) = data.get_mut(*offset) {
            *byte = if matches!(*byte, 0x0D | 0x0A) { 0x7E } else { *byte ^ 0x40 };
        }
    }
    data
}

/// In-process CNC simulator.
#[derive(Debug)]
pub struct SimulatedTransport {
    settings: SerialSettings,
    pub config: SimulationConfig,
    clock: Arc<dyn Clock>,
    open: bool,
    rate: f64,
    rx_stream: Vec<u8>,
    rx_pos: usize,
    rx_start: f64,
    opened_at: f64,
    chunk_index: usize,
    tx: Arc<Mutex<Vec<u8>>>,
    rts: bool,
    dtr: bool,
    pub open_count: usize,
    pub close_count: usize,
}

impl SimulatedTransport {
    pub fn new(settings: SerialSettings, config: SimulationConfig, clock: Arc<dyn Clock>) -> Self {
        let rate = (settings.bytes_per_second() * config.speed_factor.max(0.001)).max(1.0);
        let rx_stream = build_machine_stream(&config);
        let (rts, dtr) = (settings.initial_rts, settings.initial_dtr);
        Self {
            settings,
            config,
            clock,
            open: false,
            rate,
            rx_stream,
            rx_pos: 0,
            rx_start: 0.0,
            opened_at: 0.0,
            chunk_index: 0,
            tx: Arc::new(Mutex::new(Vec::new())),
            rts,
            dtr,
            open_count: 0,
            close_count: 0,
        }
    }

    /// Bytes the simulated CNC has received.
    pub fn machine_received(&self) -> Vec<u8> {
        self.tx.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Shared handle to the received bytes (readable after the transport was moved into a worker).
    pub fn received_handle(&self) -> Arc<Mutex<Vec<u8>>> {
        Arc::clone(&self.tx)
    }

    fn cts_now(&self) -> bool {
        match self.config.cts_on_after_s {
            Some(after) => self.clock.monotonic() - self.opened_at >= after,
            None => self.config.cts,
        }
    }

    fn require_open(&self) -> Result<(), TransportError> {
        if self.open {
            Ok(())
        } else {
            Err(TransportError::new(ErrorKind::ConnectionLost, "Simulator ist nicht geöffnet.", SIMULATED_PORT_NAME))
        }
    }

    fn available(&self) -> usize {
        let now = self.clock.monotonic();
        if now < self.rx_start {
            return 0;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let produced = ((now - self.rx_start) * self.rate + 1e-9).floor().max(0.0) as usize;
        produced.min(self.rx_stream.len()).saturating_sub(self.rx_pos)
    }

    fn write_timeout(&self) -> f64 {
        #[allow(clippy::cast_precision_loss)]
        let ms = self.settings.write_timeout_ms as f64;
        ms / 1000.0
    }
}

impl Transport for SimulatedTransport {
    fn name(&self) -> String {
        SIMULATED_PORT_NAME.into()
    }

    fn is_open(&self) -> bool {
        self.open
    }

    fn is_simulated(&self) -> bool {
        true
    }

    fn open(&mut self) -> Result<(), TransportError> {
        if self.open {
            return Ok(());
        }
        match self.config.open_error.as_deref() {
            Some("busy") => {
                return Err(TransportError::new(
                    ErrorKind::Busy,
                    "Simulierter Fehler: Port belegt (Zugriff verweigert)",
                    SIMULATED_PORT_NAME,
                ))
            }
            Some("not_found") => {
                return Err(TransportError::new(
                    ErrorKind::NotFound,
                    "Simulierter Fehler: Port existiert nicht",
                    SIMULATED_PORT_NAME,
                ))
            }
            Some(_) => {
                return Err(TransportError::new(
                    ErrorKind::Open,
                    "Simulierter Fehler beim Öffnen des Ports",
                    SIMULATED_PORT_NAME,
                ))
            }
            None => {}
        }
        self.open = true;
        self.open_count += 1;
        self.opened_at = self.clock.monotonic();
        self.rx_start = self.opened_at + self.config.machine_send_delay_s;
        self.rx_pos = 0;
        self.chunk_index = 0;
        Ok(())
    }

    fn close(&mut self) {
        if self.open {
            self.close_count += 1;
        }
        self.open = false;
    }

    fn write(&mut self, data: &[u8], cancel: &CancelToken) -> Result<usize, TransportError> {
        self.require_open()?;
        if data.is_empty() {
            return Ok(0);
        }
        let write_timeout = self.write_timeout();
        let sent = self.tx.lock().unwrap_or_else(PoisonError::into_inner).len();
        if self.settings.rts_cts && !self.cts_now() {
            self.clock.sleep(write_timeout, Some(cancel));
            if cancel.is_cancelled() {
                return Ok(0);
            }
            if !self.cts_now() {
                return Err(TransportError::new(
                    ErrorKind::Timeout,
                    "Simulierter Schreib-Timeout: CTS nicht gesetzt",
                    SIMULATED_PORT_NAME,
                ));
            }
        }
        let mut count = data.len();
        if self.config.max_write_chunk > 0 {
            count = count.min(self.config.max_write_chunk);
        }
        let limits = [
            (self.config.disconnect_after_bytes, ErrorKind::ConnectionLost),
            (self.config.write_error_after_bytes, ErrorKind::Write),
            (self.config.write_stall_after_bytes, ErrorKind::Timeout),
        ];
        for (limit, kind) in limits {
            let Some(limit) = limit else { continue };
            if sent >= limit {
                return match kind {
                    ErrorKind::ConnectionLost => {
                        self.open = false;
                        Err(TransportError::new(kind, "Simulierter Verbindungsabbruch", SIMULATED_PORT_NAME))
                    }
                    ErrorKind::Write => {
                        Err(TransportError::new(kind, "Simulierter Schreibfehler", SIMULATED_PORT_NAME))
                    }
                    _ => {
                        self.clock.sleep(write_timeout, Some(cancel));
                        if cancel.is_cancelled() {
                            return Ok(0);
                        }
                        Err(TransportError::new(kind, "Simulierter Schreib-Timeout", SIMULATED_PORT_NAME))
                    }
                };
            }
            count = count.min(limit - sent);
        }
        #[allow(clippy::cast_precision_loss)]
        if self.clock.sleep(count as f64 / self.rate, Some(cancel)) {
            return Ok(0);
        }
        let mask = u8::try_from((1u16 << self.settings.data_bits.min(8)).saturating_sub(1)).unwrap_or(0xFF);
        let mut tx = self.tx.lock().unwrap_or_else(PoisonError::into_inner);
        tx.extend(data.iter().take(count).map(|b| b & mask));
        Ok(count)
    }

    fn read(&mut self, max: usize, cancel: &CancelToken) -> Result<Vec<u8>, TransportError> {
        self.require_open()?;
        #[allow(clippy::cast_precision_loss)]
        let deadline = self.clock.monotonic() + self.settings.read_timeout_ms as f64 / 1000.0;
        loop {
            if cancel.is_cancelled() {
                return Ok(Vec::new());
            }
            if let Some(limit) = self.config.disconnect_after_bytes {
                if self.rx_pos >= limit {
                    self.open = false;
                    return Err(TransportError::new(
                        ErrorKind::ConnectionLost,
                        "Simulierter Verbindungsabbruch",
                        SIMULATED_PORT_NAME,
                    ));
                }
            }
            let available = self.available();
            if available > 0 {
                let mut count = max.min(available);
                if !self.config.read_chunk_sizes.is_empty() {
                    let index = self.chunk_index % self.config.read_chunk_sizes.len();
                    count = count.min(self.config.read_chunk_sizes.get(index).copied().unwrap_or(count).max(1));
                    self.chunk_index += 1;
                }
                if let Some(limit) = self.config.disconnect_after_bytes {
                    count = count.min(limit.saturating_sub(self.rx_pos).max(1));
                }
                let end = (self.rx_pos + count).min(self.rx_stream.len());
                let chunk = self.rx_stream.get(self.rx_pos..end).unwrap_or_default().to_vec();
                self.rx_pos = end;
                return Ok(chunk);
            }
            let now = self.clock.monotonic();
            let remaining = deadline - now;
            if remaining <= 0.0 {
                return Ok(Vec::new());
            }
            let wait = if self.rx_pos >= self.rx_stream.len() {
                remaining
            } else {
                #[allow(clippy::cast_precision_loss)]
                let next_at = self.rx_start + (self.rx_pos + 1) as f64 / self.rate;
                remaining.min((next_at - now).max(1e-4))
            };
            if self.clock.sleep(wait, Some(cancel)) {
                return Ok(Vec::new());
            }
        }
    }

    fn out_waiting(&mut self) -> usize {
        0
    }

    fn modem_status(&mut self) -> ModemStatus {
        if !self.open {
            return ModemStatus::default();
        }
        ModemStatus {
            cts: Some(self.cts_now()),
            dsr: Some(self.config.dsr),
            dcd: Some(self.config.dcd),
            ri: Some(self.config.ri),
            rts: Some(self.rts),
            dtr: Some(self.dtr),
        }
    }

    fn set_control_lines(&mut self, rts: Option<bool>, dtr: Option<bool>) -> Result<(), TransportError> {
        self.require_open()?;
        if let Some(rts) = rts {
            self.rts = rts;
        }
        if let Some(dtr) = dtr {
            self.dtr = dtr;
        }
        Ok(())
    }

    fn reset_input(&mut self) {
        self.rx_pos += self.available();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FakeClock;

    fn sim(config: SimulationConfig) -> (SimulatedTransport, FakeClock) {
        let clock = FakeClock::default();
        (SimulatedTransport::new(SerialSettings::default(), config, Arc::new(clock.clone())), clock)
    }

    #[test]
    fn write_masks_and_times() {
        let (mut t, clock) = sim(SimulationConfig::default());
        let token = CancelToken::new();
        assert!(t.write(b"x", &token).is_err());
        t.open().unwrap();
        assert_eq!(t.write(b"AB\xc4", &token).unwrap(), 3);
        assert_eq!(t.machine_received(), vec![b'A', b'B', 0x44]);
        assert!((clock.monotonic() - 3.0 * 9.0 / 4800.0).abs() < 1e-9);
        assert_eq!(t.modem_status().cts, Some(true));
        t.set_control_lines(Some(false), Some(true)).unwrap();
        assert_eq!(t.modem_status().rts, Some(false));
        t.close();
        assert_eq!(t.modem_status(), ModemStatus::default());
        assert_eq!((t.open_count, t.close_count), (1, 1));
    }

    #[test]
    fn read_delivers_machine_program() {
        let config = SimulationConfig {
            machine_program: b"N10 M30\r\n".to_vec(),
            read_chunk_sizes: vec![3],
            inject_null_every: 4,
            corrupt_offsets: vec![0, 99],
            ..SimulationConfig::default()
        };
        let (mut t, _clock) = sim(config);
        t.open().unwrap();
        let token = CancelToken::new();
        let mut all = Vec::new();
        loop {
            let chunk = t.read(256, &token).unwrap();
            if chunk.is_empty() {
                break;
            }
            assert!(chunk.len() <= 3);
            all.extend(chunk);
        }
        assert_eq!(all, b"\x0e10 \x00M30\r\x00\n".to_vec());
    }

    #[test]
    fn faults() {
        for (config, kind) in [
            (SimulationConfig { open_error: Some("busy".into()), ..SimulationConfig::default() }, ErrorKind::Busy),
            (
                SimulationConfig { open_error: Some("not_found".into()), ..SimulationConfig::default() },
                ErrorKind::NotFound,
            ),
            (SimulationConfig { open_error: Some("x".into()), ..SimulationConfig::default() }, ErrorKind::Open),
        ] {
            let (mut t, _) = sim(config);
            assert_eq!(t.open().unwrap_err().kind, kind);
        }
        let token = CancelToken::new();
        let (mut t, _) = sim(SimulationConfig { write_error_after_bytes: Some(2), ..SimulationConfig::default() });
        t.open().unwrap();
        assert_eq!(t.write(b"ABCD", &token).unwrap(), 2);
        assert_eq!(t.write(b"CD", &token).unwrap_err().kind, ErrorKind::Write);
        let (mut t, _) = sim(SimulationConfig { disconnect_after_bytes: Some(1), ..SimulationConfig::default() });
        t.open().unwrap();
        t.write(b"A", &token).unwrap();
        assert_eq!(t.write(b"B", &token).unwrap_err().kind, ErrorKind::ConnectionLost);
        assert!(!t.is_open());
        let (mut t, _) = sim(SimulationConfig { write_stall_after_bytes: Some(0), ..SimulationConfig::default() });
        t.open().unwrap();
        assert_eq!(t.write(b"A", &token).unwrap_err().kind, ErrorKind::Timeout);
        let mut settings = SerialSettings { rts_cts: true, ..SerialSettings::default() };
        settings.write_timeout_ms = 100;
        let clock = FakeClock::default();
        let mut t = SimulatedTransport::new(
            settings,
            SimulationConfig { cts: false, ..SimulationConfig::default() },
            Arc::new(clock),
        );
        t.open().unwrap();
        assert_eq!(t.write(b"A", &token).unwrap_err().kind, ErrorKind::Timeout);
        let cancelled = CancelToken::new();
        cancelled.cancel();
        assert_eq!(t.write(b"A", &cancelled).unwrap(), 0);
        assert!(t.read(10, &cancelled).unwrap().is_empty());
        t.reset_input();
    }
}
