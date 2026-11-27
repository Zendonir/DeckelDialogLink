//! Sending NC data to the CNC.
//!
//! The sender only transmits bytes. It never sends execution commands and never
//! toggles anything that could start a program on the machine.

use crate::clock::{CancelToken, Clock};
use crate::model::Profile;
use crate::nc::TransformResult;
use crate::storage::sha256_hex;
use crate::transfer::{Direction, Event, EventSink, ProgressTracker, SendResult, SendState, TrafficEvent};
use crate::transport::{describe, ErrorKind, ModemStatus, Transport, TransportError};

pub const DEFAULT_CHUNK: usize = 64;
pub const POLL_INTERVAL_S: f64 = 0.05;

enum Stop {
    Cancelled,
    Failed(TransportError),
}

impl From<TransportError> for Stop {
    fn from(err: TransportError) -> Self {
        Stop::Failed(err)
    }
}

pub struct ProgramSender<'a> {
    transport: &'a mut dyn Transport,
    profile: &'a Profile,
    clock: &'a dyn Clock,
    token: &'a CancelToken,
    events: &'a mut dyn EventSink,
    pub state: SendState,
}

impl std::fmt::Debug for ProgramSender<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProgramSender").field("state", &self.state).finish_non_exhaustive()
    }
}

#[allow(clippy::cast_precision_loss)]
fn ms(value: u64) -> f64 {
    value as f64 / 1000.0
}

impl<'a> ProgramSender<'a> {
    pub fn new(
        transport: &'a mut dyn Transport,
        profile: &'a Profile,
        clock: &'a dyn Clock,
        token: &'a CancelToken,
        events: &'a mut dyn EventSink,
    ) -> Self {
        Self { transport, profile, clock, token, events, state: SendState::Ready }
    }

    fn set_state(&mut self, state: SendState) {
        self.state = state;
        self.events.emit(Event::SendState(state));
    }

    fn check(&self) -> Result<(), Stop> {
        if self.token.is_cancelled() {
            Err(Stop::Cancelled)
        } else {
            Ok(())
        }
    }

    fn sleep_ms(&self, milliseconds: u64) -> Result<(), Stop> {
        if milliseconds == 0 {
            return self.check();
        }
        if self.clock.sleep(ms(milliseconds), Some(self.token)) {
            return Err(Stop::Cancelled);
        }
        Ok(())
    }

    fn emit_modem(&mut self) -> ModemStatus {
        let status = self.transport.modem_status();
        self.events.emit(Event::Modem(status));
        status
    }

    /// Send `payload`. Never panics; every failure ends in a `Failed` result with a German description.
    pub fn send(&mut self, payload: &TransformResult) -> SendResult {
        let started_at = self.clock.now();
        let t0 = self.clock.monotonic();
        let mut tracker = ProgressTracker::new(payload);
        let mut error = None;
        let outcome = self.run(payload, &mut tracker);
        match outcome {
            Ok(()) => self.set_state(SendState::Completed),
            Err(Stop::Cancelled) => self.set_state(SendState::Cancelled),
            Err(Stop::Failed(err)) => {
                error = Some(describe(&err, "Senden", &self.profile.id));
                self.set_state(SendState::Failed);
            }
        }
        let keep_open = self.state == SendState::Completed && !self.profile.transfer.close_port_after_send;
        let port_left_open = keep_open && self.transport.is_open();
        if !port_left_open {
            self.transport.close();
        }
        SendResult {
            state: self.state,
            bytes_total: payload.data.len(),
            bytes_sent: tracker.bytes_done,
            started_at,
            duration_s: self.clock.monotonic() - t0,
            sha256: sha256_hex(&payload.data),
            profile_id: self.profile.id.clone(),
            port: self.transport.name(),
            simulated: self.transport.is_simulated(),
            error,
            port_left_open,
        }
    }

    fn run(&mut self, payload: &TransformResult, tracker: &mut ProgressTracker<'_>) -> Result<(), Stop> {
        self.check()?;
        self.set_state(SendState::OpeningPort);
        if !self.transport.is_open() {
            self.transport.open()?;
        }
        self.emit_modem();
        let serial = &self.profile.serial;
        if serial.require_cts_before_send || serial.require_dsr_before_send {
            self.set_state(SendState::WaitingForHandshake);
            self.wait_for_handshake()?;
        }
        self.sleep_ms(self.profile.transfer.start_delay_ms)?;
        self.set_state(SendState::Sending);
        tracker.start(self.clock.monotonic());
        self.events.emit(Event::Progress(tracker.snapshot(self.clock.monotonic())));
        self.send_bytes(&payload.data, tracker)?;
        self.set_state(SendState::Finishing);
        self.drain()?;
        self.sleep_ms(self.profile.transfer.end_delay_ms)?;
        self.emit_modem();
        Ok(())
    }

    fn wait_for_handshake(&mut self) -> Result<(), Stop> {
        let serial = self.profile.serial.clone();
        let deadline = self.clock.monotonic() + ms(serial.handshake_timeout_ms);
        loop {
            let status = self.emit_modem();
            let mut missing = Vec::new();
            if serial.require_cts_before_send && status.cts != Some(true) {
                missing.push("CTS".to_string());
            }
            if serial.require_dsr_before_send && status.dsr != Some(true) {
                missing.push("DSR".to_string());
            }
            if missing.is_empty() {
                return Ok(());
            }
            if self.clock.monotonic() >= deadline {
                let mut err = TransportError::new(
                    ErrorKind::HandshakeTimeout,
                    format!("{} nicht aktiv nach {} ms", missing.join("/"), serial.handshake_timeout_ms),
                    self.transport.name(),
                );
                err.missing = missing;
                return Err(Stop::Failed(err));
            }
            if self.clock.sleep(POLL_INTERVAL_S, Some(self.token)) {
                return Err(Stop::Cancelled);
            }
        }
    }

    fn send_bytes(&mut self, data: &[u8], tracker: &mut ProgressTracker<'_>) -> Result<(), Stop> {
        let settings = self.profile.transfer.clone();
        let packet = usize::try_from(settings.packet_size).unwrap_or(0);
        let unit = if settings.character_delay_ms > 0 {
            1
        } else if packet > 0 {
            packet
        } else {
            DEFAULT_CHUNK
        };
        let mut offset = 0usize;
        while offset < data.len() {
            self.check()?;
            let end = (offset + unit).min(data.len());
            self.write_all(data.get(offset..end).unwrap_or_default(), tracker)?;
            offset = end;
            if offset >= data.len() {
                break;
            }
            if settings.character_delay_ms > 0 {
                self.sleep_ms(settings.character_delay_ms)?;
            }
            if packet > 0 && settings.packet_delay_ms > 0 && offset.is_multiple_of(packet) {
                self.sleep_ms(settings.packet_delay_ms)?;
            }
        }
        Ok(())
    }

    fn write_all(&mut self, block: &[u8], tracker: &mut ProgressTracker<'_>) -> Result<(), Stop> {
        let timeout = ms(self.profile.serial.write_timeout_ms);
        let mut pos = 0usize;
        let mut stall_since: Option<f64> = None;
        while pos < block.len() {
            self.check()?;
            let rest = block.get(pos..).unwrap_or_default();
            let written = self.transport.write(rest, self.token)?.min(rest.len());
            self.check()?;
            if written > 0 {
                let chunk = rest.get(..written).unwrap_or_default().to_vec();
                pos += written;
                stall_since = None;
                self.events.emit(Event::Traffic(TrafficEvent {
                    direction: Direction::Tx,
                    timestamp: self.clock.now(),
                    data: chunk,
                }));
                let progress = tracker.advance(written, self.clock.monotonic());
                self.events.emit(Event::Progress(progress));
                continue;
            }
            let now = self.clock.monotonic();
            match stall_since {
                None => stall_since = Some(now),
                Some(since) if now - since >= timeout => {
                    return Err(Stop::Failed(TransportError::new(
                        ErrorKind::Timeout,
                        format!("Keine Daten mehr angenommen seit {timeout:.1} s"),
                        self.transport.name(),
                    )));
                }
                Some(_) => {}
            }
            if self.clock.sleep(POLL_INTERVAL_S, Some(self.token)) {
                return Err(Stop::Cancelled);
            }
        }
        Ok(())
    }

    fn drain(&mut self) -> Result<(), Stop> {
        let deadline = self.clock.monotonic() + ms(self.profile.serial.write_timeout_ms);
        while self.transport.out_waiting() > 0 {
            if self.clock.monotonic() >= deadline {
                return Err(Stop::Failed(TransportError::new(
                    ErrorKind::Timeout,
                    "Ausgangspuffer wurde nicht geleert (Handshake blockiert?)",
                    self.transport.name(),
                )));
            }
            if self.clock.sleep(POLL_INTERVAL_S, Some(self.token)) {
                return Err(Stop::Cancelled);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FakeClock;
    use crate::nc::transformer::transform;
    use crate::profile::builtin_profiles;
    use crate::transport::simulated::{SimulatedTransport, SimulationConfig};
    use std::sync::Arc;

    const PROGRAM: &[u8] = b"(DECKELLINK TEST)\r\nN10 G00 X0 Y0\r\nN20 G01 X10 F100\r\nN30 M30\r\n";

    fn profile(id: &str) -> Profile {
        builtin_profiles().0.into_iter().find(|p| p.id == id).unwrap()
    }

    fn run(
        profile: &Profile,
        config: SimulationConfig,
        token: &CancelToken,
    ) -> (SendResult, Vec<u8>, Vec<Event>, FakeClock) {
        let clock = FakeClock::default();
        let mut transport = SimulatedTransport::new(profile.serial.clone(), config, Arc::new(clock.clone()));
        let payload = transform(PROGRAM, &profile.transfer, false).unwrap();
        let mut events: Vec<Event> = Vec::new();
        let result = ProgramSender::new(&mut transport, profile, &clock, token, &mut events).send(&payload);
        (result, transport.machine_received(), events, clock)
    }

    #[test]
    fn sends_complete_program() {
        let p = profile("dialog11_7n1_rtscts");
        let (result, received, events, clock) = run(&p, SimulationConfig::default(), &CancelToken::new());
        assert_eq!(result.state, SendState::Completed, "{:?}", result.error);
        assert_eq!(received, PROGRAM);
        assert_eq!(result.bytes_sent, PROGRAM.len());
        assert!(result.simulated && !result.port_left_open);
        let states: Vec<_> =
            events.iter().filter_map(|e| if let Event::SendState(s) = e { Some(*s) } else { None }).collect();
        assert_eq!(
            states,
            vec![
                SendState::OpeningPort,
                SendState::WaitingForHandshake,
                SendState::Sending,
                SendState::Finishing,
                SendState::Completed
            ]
        );
        // start + end delay (2 s) plus line time at 4800 baud 7N1
        let expected = 2.0 + PROGRAM.len() as f64 * 9.0 / 4800.0;
        assert!((clock.monotonic() - expected).abs() < 1e-6);
        assert!(events.iter().any(|e| matches!(e, Event::Progress(p) if p.current_line == 4)));
    }

    #[test]
    fn handshake_timeout_and_late_cts() {
        let p = profile("dialog11_7n1_rtscts");
        let (result, _, _, _) =
            run(&p, SimulationConfig { cts: false, ..SimulationConfig::default() }, &CancelToken::new());
        assert_eq!(result.state, SendState::Failed);
        assert!(result.error.unwrap().title.contains("Sendefreigabe (CTS)"));
        let late = SimulationConfig { cts: false, cts_on_after_s: Some(3.0), ..SimulationConfig::default() };
        let (result, received, _, _) = run(&p, late, &CancelToken::new());
        assert_eq!(result.state, SendState::Completed);
        assert_eq!(received, PROGRAM);
    }

    #[test]
    fn faults_are_reported_not_panicking() {
        let p = profile("dialog11_7e1_none");
        for (config, text) in [
            (SimulationConfig { open_error: Some("busy".into()), ..SimulationConfig::default() }, "belegt"),
            (SimulationConfig { disconnect_after_bytes: Some(20), ..SimulationConfig::default() }, "unterbrochen"),
            (SimulationConfig { write_error_after_bytes: Some(10), ..SimulationConfig::default() }, "Schreibfehler"),
            (
                SimulationConfig { write_stall_after_bytes: Some(10), ..SimulationConfig::default() },
                "Zeitüberschreitung",
            ),
        ] {
            let (result, _, _, _) = run(&p, config, &CancelToken::new());
            assert_eq!(result.state, SendState::Failed);
            let error = result.error.unwrap();
            assert!(error.title.contains(text), "{}", error.title);
        }
    }

    #[test]
    fn cancel_and_pacing_options() {
        let p = profile("dialog11_7e1_none");
        let token = CancelToken::new();
        token.cancel();
        let (result, received, _, _) = run(&p, SimulationConfig::default(), &token);
        assert_eq!(result.state, SendState::Cancelled);
        assert!(received.is_empty());
        let mut paced = p.clone();
        paced.transfer.character_delay_ms = 2;
        paced.transfer.close_port_after_send = false;
        let (result, received, _, _) =
            run(&paced, SimulationConfig { max_write_chunk: 1, ..SimulationConfig::default() }, &CancelToken::new());
        assert_eq!(result.state, SendState::Completed);
        assert!(result.port_left_open);
        assert_eq!(received, PROGRAM);
        let mut packets = p;
        packets.transfer.packet_size = 8;
        packets.transfer.packet_delay_ms = 100;
        let (result, _, _, clock) = run(&packets, SimulationConfig::default(), &CancelToken::new());
        assert_eq!(result.state, SendState::Completed);
        assert!(clock.monotonic() > 2.5);
    }
}
