//! Receiving NC data from the CNC (plain protocol).

use crate::clock::{CancelToken, Clock};
use crate::error::UserFacingError;
use crate::model::{LineEnding, Profile};
use crate::nc::analyzer::analyze;
use crate::nc::checks::{split_lines, ProgramEndMatcher, EOT};
use crate::nc::AnalysisResult;
use crate::transfer::{
    Direction, EndReason, Event, EventSink, ReceiveProgress, ReceiveResult, ReceiveState, TrafficEvent,
};
use crate::transport::{describe, Transport, TransportError};

pub const READ_SIZE: usize = 256;
const SEARCH_OVERLAP: usize = 64;

/// Detects the configured end sequence (default `M30`) incrementally; NUL bytes are ignored.
#[derive(Debug, Clone)]
pub struct EndSequenceDetector {
    matcher: Option<ProgramEndMatcher>,
    scanned: usize,
    pub match_end: Option<usize>,
}

impl EndSequenceDetector {
    pub fn new(sequence: &str) -> Self {
        Self { matcher: ProgramEndMatcher::new(sequence), scanned: 0, match_end: None }
    }

    pub fn enabled(&self) -> bool {
        self.matcher.is_some()
    }

    /// True once the sequence was found anywhere in `buffer`.
    pub fn feed(&mut self, buffer: &[u8]) -> bool {
        let Some(matcher) = &self.matcher else {
            return false;
        };
        if self.match_end.is_some() {
            return true;
        }
        let start = self.scanned.saturating_sub(SEARCH_OVERLAP);
        let window: Vec<u8> =
            buffer.get(start..).unwrap_or_default().iter().map(|b| if *b == 0 { 1 } else { *b }).collect();
        self.scanned = buffer.len();
        let Some((m_start, m_end)) = matcher.find(&window) else {
            return false;
        };
        if m_end == window.len() {
            // a following digit could still turn "M30" into "M300" - wait for more data
            self.scanned = start + m_start;
            return false;
        }
        self.match_end = Some(start + m_end);
        true
    }

    /// True if the profile's line ending follows the detected sequence (CR suffices for CR profiles).
    pub fn line_terminated(&self, buffer: &[u8], line_ending: LineEnding) -> bool {
        let Some(end) = self.match_end else {
            return false;
        };
        let tail: Vec<u8> = buffer.get(end..).unwrap_or_default().iter().copied().filter(|b| *b != 0).collect();
        if line_ending == LineEnding::Cr {
            tail.contains(&b'\r') || tail.contains(&b'\n')
        } else {
            tail.contains(&b'\n')
        }
    }
}

/// `[\0\r\n]* EOT` at the start of `tail`; returns (length incl. optional line end, line end seen).
fn eot_after_end(tail: &[u8], final_check: bool) -> Option<(usize, bool)> {
    let lead = tail.iter().take_while(|b| matches!(**b, 0 | b'\r' | b'\n')).count();
    if tail.get(lead) != Some(&EOT) {
        return None;
    }
    let after = lead + 1;
    match (tail.get(after), tail.get(after + 1)) {
        (Some(b'\r'), Some(b'\n')) => Some((after + 2, true)),
        (Some(b'\n'), _) => Some((after + 1, true)),
        (Some(b'\r'), _) if final_check => Some((after + 1, true)),
        _ => Some((after, false)),
    }
}

enum Stop {
    Cancelled,
    Failed(TransportError),
}

impl From<TransportError> for Stop {
    fn from(err: TransportError) -> Self {
        Stop::Failed(err)
    }
}

pub struct ProgramReceiver<'a> {
    transport: &'a mut dyn Transport,
    profile: &'a Profile,
    clock: &'a dyn Clock,
    token: &'a CancelToken,
    events: &'a mut dyn EventSink,
    pub read_size: usize,
    pub state: ReceiveState,
}

impl std::fmt::Debug for ProgramReceiver<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProgramReceiver").field("state", &self.state).finish_non_exhaustive()
    }
}

#[allow(clippy::cast_precision_loss)]
fn ms(value: u64) -> f64 {
    value as f64 / 1000.0
}

struct Outcome {
    end_reason: EndReason,
    state: ReceiveState,
    error: Option<UserFacingError>,
}

impl<'a> ProgramReceiver<'a> {
    pub fn new(
        transport: &'a mut dyn Transport,
        profile: &'a Profile,
        clock: &'a dyn Clock,
        token: &'a CancelToken,
        events: &'a mut dyn EventSink,
    ) -> Self {
        Self { transport, profile, clock, token, events, read_size: READ_SIZE, state: ReceiveState::Idle }
    }

    fn set_state(&mut self, state: ReceiveState) {
        self.state = state;
        self.events.emit(Event::ReceiveState(state));
    }

    fn traffic(&mut self, data: &[u8]) {
        if !data.is_empty() {
            self.events.emit(Event::Traffic(TrafficEvent {
                direction: Direction::Rx,
                timestamp: self.clock.now(),
                data: data.to_vec(),
            }));
        }
    }

    fn progress(&mut self, count: usize, elapsed: f64, idle: f64) {
        let state = self.state;
        self.events.emit(Event::ReceiveProgress(ReceiveProgress {
            state,
            bytes_received: count,
            elapsed_s: elapsed,
            idle_s: idle,
        }));
    }

    fn check(&self) -> Result<(), Stop> {
        if self.token.is_cancelled() {
            Err(Stop::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Receive until end sequence, EOT, inactivity, size limit, timeout or cancel. Never panics.
    pub fn receive(&mut self) -> ReceiveResult {
        let started_at = self.clock.now();
        let t0 = self.clock.monotonic();
        let mut buffer: Vec<u8> = Vec::new();
        let mut first_at = None;
        let mut last_at = None;
        let outcome = match self.run(&mut buffer, &mut first_at, &mut last_at, t0) {
            Ok(outcome) => outcome,
            Err(Stop::Cancelled) => {
                Outcome { end_reason: EndReason::Cancelled, state: ReceiveState::Cancelled, error: None }
            }
            Err(Stop::Failed(err)) => Outcome {
                end_reason: EndReason::Error,
                state: ReceiveState::Failed,
                error: Some(describe(&err, "Empfangen", &self.profile.id)),
            },
        };
        self.transport.close();
        self.set_state(outcome.state);
        ReceiveResult {
            state: outcome.state,
            raw: buffer,
            end_reason: outcome.end_reason,
            started_at,
            duration_s: self.clock.monotonic() - t0,
            profile_id: self.profile.id.clone(),
            port: self.transport.name(),
            simulated: self.transport.is_simulated(),
            first_byte_at: first_at,
            last_byte_at: last_at,
            error: outcome.error,
        }
    }

    #[allow(clippy::too_many_lines)]
    fn run(
        &mut self,
        buffer: &mut Vec<u8>,
        first_at: &mut Option<chrono::DateTime<chrono::FixedOffset>>,
        last_at: &mut Option<chrono::DateTime<chrono::FixedOffset>>,
        t0: f64,
    ) -> Result<Outcome, Stop> {
        let settings = self.profile.receive.clone();
        let line_ending = self.profile.transfer.line_ending;
        let mut detector = EndSequenceDetector::new(&settings.end_sequence);
        let mut end_seen: Option<f64> = None;
        let mut eot_seen: Option<f64> = None;
        let mut eot_index = 0usize;
        let mut last_mono: Option<f64> = None;
        let grace = ms(settings.end_grace_ms);
        let max_bytes = usize::try_from(settings.max_receive_bytes).unwrap_or(usize::MAX);
        let done = |reason, state| Ok(Outcome { end_reason: reason, state, error: None });

        self.check()?;
        self.set_state(ReceiveState::OpeningPort);
        if !self.transport.is_open() {
            self.transport.open()?;
        }
        let status = self.transport.modem_status();
        self.events.emit(Event::Modem(status));
        self.set_state(ReceiveState::Listening);
        let listen_start = self.clock.monotonic();
        loop {
            self.check()?;
            let chunk = self.transport.read(self.read_size, self.token)?;
            self.check()?;
            let now = self.clock.monotonic();
            if !chunk.is_empty() {
                let wall = self.clock.now();
                if first_at.is_none() {
                    *first_at = Some(wall);
                    self.set_state(ReceiveState::Receiving);
                }
                *last_at = Some(wall);
                last_mono = Some(now);
                if max_bytes > 0 && buffer.len() + chunk.len() > max_bytes {
                    let room = max_bytes.saturating_sub(buffer.len());
                    let part = chunk.get(..room).unwrap_or_default();
                    buffer.extend_from_slice(part);
                    self.traffic(part);
                    return Ok(Outcome {
                        end_reason: EndReason::MaxSize,
                        state: ReceiveState::Failed,
                        error: Some(UserFacingError {
                            title: "Maximale Empfangsgröße überschritten - Empfang gestoppt.".into(),
                            causes: vec![
                                format!("Grenze laut Profil: {} Bytes.", settings.max_receive_bytes),
                                "Die CNC sendet mehr Daten als erwartet oder sendet fortlaufend.".into(),
                            ],
                            technical_details: format!("received>{}", settings.max_receive_bytes),
                            operation: "Empfangen".into(),
                        }),
                    });
                }
                buffer.extend_from_slice(&chunk);
                self.traffic(&chunk);
                self.progress(buffer.len(), now - t0, 0.0);
                if settings.end_on_eot && eot_seen.is_none() {
                    if let Some(pos) = chunk.iter().position(|b| *b == EOT) {
                        eot_seen = Some(now);
                        eot_index = buffer.len() - chunk.len() + pos;
                    }
                }
                if let Some(eot_at) = eot_seen {
                    let after = buffer.get(eot_index + 1..).unwrap_or_default();
                    if after.contains(&b'\n') || now - eot_at >= grace {
                        return done(EndReason::Eot, ReceiveState::Completed);
                    }
                    continue;
                }
                if settings.end_detection.uses_end_sequence() && end_seen.is_none() && detector.feed(buffer) {
                    end_seen = Some(now);
                }
                if end_seen.is_some() && detector.line_terminated(buffer, line_ending) {
                    let reason = if settings.end_on_eot && self.capture_trailing_eot(buffer, grace)? {
                        EndReason::Eot
                    } else {
                        EndReason::EndSequence
                    };
                    return done(reason, ReceiveState::Completed);
                }
                if end_seen.is_some_and(|seen| now - seen >= grace) {
                    return done(EndReason::EndSequence, ReceiveState::Completed);
                }
                continue;
            }
            let Some(last) = last_mono else {
                if settings.first_byte_timeout_ms > 0 && now - listen_start >= ms(settings.first_byte_timeout_ms) {
                    return done(EndReason::NoData, ReceiveState::TimedOut);
                }
                self.progress(0, now - t0, now - listen_start);
                continue;
            };
            let idle = now - last;
            self.progress(buffer.len(), now - t0, idle);
            if eot_seen.is_some_and(|at| now - at >= grace) {
                return done(EndReason::Eot, ReceiveState::Completed);
            }
            if end_seen.is_some_and(|seen| now - seen >= grace) {
                return done(EndReason::EndSequence, ReceiveState::Completed);
            }
            if idle >= ms(settings.inactivity_timeout_ms) {
                let state = if settings.end_detection.uses_inactivity() {
                    ReceiveState::Completed
                } else {
                    ReceiveState::TimedOut
                };
                return done(EndReason::Inactivity, state);
            }
        }
    }

    /// After the program end, take over an immediately following EOT (+ line end); other data is not read.
    fn capture_trailing_eot(&mut self, buffer: &mut Vec<u8>, grace: f64) -> Result<bool, Stop> {
        let deadline = self.clock.monotonic() + grace;
        let mut tail: Vec<u8> = Vec::new();
        while self.clock.monotonic() < deadline {
            self.check()?;
            let chunk = self.transport.read(16, self.token)?;
            if chunk.is_empty() {
                continue;
            }
            tail.extend_from_slice(&chunk);
            self.traffic(&chunk);
            match eot_after_end(&tail, false) {
                None => {
                    if tail.iter().any(|b| !matches!(*b, 0 | b'\r' | b'\n')) {
                        return Ok(false);
                    }
                }
                Some((len, true)) => {
                    buffer.extend_from_slice(tail.get(..len).unwrap_or_default());
                    return Ok(true);
                }
                Some((_, false)) => {}
            }
        }
        if let Some((len, _)) = eot_after_end(&tail, true) {
            buffer.extend_from_slice(tail.get(..len).unwrap_or_default());
            return Ok(true);
        }
        Ok(false)
    }
}

/// Post-processed receive data (raw bytes stay untouched).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceivedProgram {
    pub raw: Vec<u8>,
    pub normalized: Vec<u8>,
    pub analysis: AnalysisResult,
    pub complete: bool,
    pub decode_errors: usize,
    pub notes: Vec<String>,
}

pub fn normalize_received(raw: &[u8], profile: &Profile) -> Vec<u8> {
    let settings = &profile.receive;
    let data: Vec<u8> =
        if settings.strip_null_bytes { raw.iter().copied().filter(|b| *b != 0).collect() } else { raw.to_vec() };
    if !settings.normalize_line_endings {
        return data;
    }
    let target = profile.transfer.line_ending.sequence();
    let mut out = Vec::with_capacity(data.len());
    for line in split_lines(&data) {
        out.extend_from_slice(&line.content);
        if !line.ending.is_empty() {
            out.extend_from_slice(target);
        }
    }
    out
}

/// Normalise and analyse received raw bytes.
pub fn process_received(result: &ReceiveResult, profile: &Profile) -> ReceivedProgram {
    let raw = result.raw.clone();
    let normalized = normalize_received(&raw, profile);
    let analysis = analyze(&normalized, &profile.transfer, Some(&profile.serial));
    let decode_errors = raw.iter().filter(|b| **b > 0x7F).count();
    let mut notes = Vec::new();
    if decode_errors > 0 {
        notes.push(format!(
            "{decode_errors} Byte(s) außerhalb 7-Bit-ASCII empfangen - möglicher Paritäts- oder Datenbitfehler (Profil prüfen)."
        ));
    }
    let nulls = raw.iter().filter(|b| **b == 0).count();
    if nulls > 0 {
        let suffix = if profile.receive.strip_null_bytes { " - in der NC-Datei entfernt." } else { "." };
        notes.push(format!("{nulls} Nullbyte(s) im Rohdatenstrom{suffix}"));
    }
    let complete = result.state == ReceiveState::Completed
        && matches!(result.end_reason, EndReason::EndSequence | EndReason::Inactivity | EndReason::Eot)
        && (analysis.has_program_end || profile.transfer.program_end.trim().is_empty());
    if !complete {
        notes.push("Programm ist möglicherweise UNVOLLSTÄNDIG - nicht ungeprüft verwenden.".into());
    }
    if profile.transfer.line_ending != LineEnding::Crlf && profile.receive.normalize_line_endings {
        notes.push(format!("Zeilenenden auf {} normalisiert.", profile.transfer.line_ending.as_str().to_uppercase()));
    }
    ReceivedProgram { raw, normalized, analysis, complete, decode_errors, notes }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FakeClock;
    use crate::model::EndDetection;
    use crate::profile::builtin_profiles;
    use crate::transport::simulated::{SimulatedTransport, SimulationConfig};
    use std::sync::Arc;

    const PROGRAM: &[u8] = b"(DECKELLINK TEST)\r\nN10 G00 X0 Y0\r\nN20 G01 X10 F100\r\nN30 M30\r\n";

    fn profile() -> Profile {
        builtin_profiles().0.into_iter().find(|p| p.id == "dialog11_7n1_rtscts").unwrap()
    }

    fn receive(profile: &Profile, config: SimulationConfig) -> (ReceiveResult, Vec<Event>) {
        let clock = FakeClock::default();
        let mut transport = SimulatedTransport::new(profile.serial.clone(), config, Arc::new(clock.clone()));
        let token = CancelToken::new();
        let mut events: Vec<Event> = Vec::new();
        let result = ProgramReceiver::new(&mut transport, profile, &clock, &token, &mut events).receive();
        (result, events)
    }

    fn machine(data: &[u8]) -> SimulationConfig {
        SimulationConfig { machine_program: data.to_vec(), ..SimulationConfig::default() }
    }

    #[test]
    fn until_program_end_ignoring_garbage() {
        let mut data = PROGRAM.to_vec();
        data.extend_from_slice(&b"GARBAGE AFTER\r\n".repeat(5));
        let (result, events) = receive(&profile(), machine(&data));
        assert_eq!((result.state, result.end_reason), (ReceiveState::Completed, EndReason::EndSequence));
        assert!(result.raw.starts_with(PROGRAM) && result.raw.ends_with(b"M30\r\n"));
        let states: Vec<_> =
            events.iter().filter_map(|e| if let Event::ReceiveState(s) = e { Some(*s) } else { None }).collect();
        assert_eq!(states.first(), Some(&ReceiveState::OpeningPort));
        assert_eq!(states.last(), Some(&ReceiveState::Completed));
        assert!(process_received(&result, &profile()).complete);
    }

    #[test]
    fn eot_variants() {
        let mut data = PROGRAM.to_vec();
        data.extend_from_slice(b"\x04\r\n");
        let (result, _) = receive(&profile(), machine(&data));
        assert_eq!(result.end_reason, EndReason::Eot);
        assert_eq!(result.raw, data);
        let params = b"%MPG00 (KOPF)\r\nM65000 X\r\n?\r\n1234\r\n\x04\r\n";
        let (result, _) = receive(&profile(), machine(params));
        assert_eq!((result.end_reason, result.raw.as_slice()), (EndReason::Eot, &params[..]));
        assert!(!process_received(&result, &profile()).complete);
        let mut no_eot = profile();
        no_eot.receive.end_on_eot = false;
        let (result, _) = receive(&no_eot, machine(b"N10 G00\r\n\x04\r\n"));
        assert_ne!(result.end_reason, EndReason::Eot);
        assert_eq!(result.raw, b"N10 G00\r\n\x04\r\n");
    }

    #[test]
    fn timeouts_limits_and_chunking() {
        let mut p = profile();
        p.receive.first_byte_timeout_ms = 2000;
        let (result, _) = receive(&p, machine(b""));
        assert_eq!((result.state, result.end_reason), (ReceiveState::TimedOut, EndReason::NoData));
        let (result, _) = receive(&profile(), machine(b"N10 G00\r\nN20 G01\r\n"));
        assert_eq!((result.state, result.end_reason), (ReceiveState::Completed, EndReason::Inactivity));
        assert!(!process_received(&result, &profile()).complete);
        let mut seq_only = profile();
        seq_only.receive.end_detection = EndDetection::EndSequence;
        let (result, _) = receive(&seq_only, machine(b"N10 G00\r\n"));
        assert_eq!(result.state, ReceiveState::TimedOut);
        let mut small = profile();
        small.receive.max_receive_bytes = 10;
        let (result, _) = receive(&small, machine(PROGRAM));
        assert_eq!((result.end_reason, result.raw.len()), (EndReason::MaxSize, 10));
        assert!(result.error.is_some());
        let chunked = SimulationConfig { read_chunk_sizes: vec![1, 2, 3], inject_null_every: 5, ..machine(PROGRAM) };
        let (result, _) = receive(&profile(), chunked);
        assert_eq!(result.end_reason, EndReason::EndSequence);
        let program = process_received(&result, &profile());
        assert_eq!(program.normalized, PROGRAM);
        assert!(program.notes.iter().any(|n| n.contains("Nullbyte")));
        let busy = SimulationConfig { open_error: Some("busy".into()), ..SimulationConfig::default() };
        let (result, _) = receive(&profile(), busy);
        assert_eq!((result.state, result.end_reason), (ReceiveState::Failed, EndReason::Error));
        let lost = SimulationConfig { disconnect_after_bytes: Some(10), ..machine(PROGRAM) };
        let (result, _) = receive(&profile(), lost);
        assert_eq!(result.state, ReceiveState::Failed);
    }

    #[test]
    fn m30_split_across_chunks_and_m300() {
        let chunked = SimulationConfig { read_chunk_sizes: vec![1], ..machine(b"N10 M3\r\nN20 M300\r\nN30 M30\r\n") };
        let (result, _) = receive(&profile(), chunked);
        assert_eq!(result.end_reason, EndReason::EndSequence);
        assert!(result.raw.ends_with(b"N30 M30\r\n"));
    }

    #[test]
    fn detector_and_helpers() {
        let mut d = EndSequenceDetector::new("M30");
        assert!(d.enabled());
        assert!(!d.feed(b"N10 M3"));
        assert!(!d.feed(b"N10 M30"));
        assert!(d.feed(b"N10 M30\r"));
        assert!(!d.line_terminated(b"N10 M30\r", LineEnding::Crlf));
        assert!(d.line_terminated(b"N10 M30\r", LineEnding::Cr));
        assert!(d.feed(b"N10 M30\r\n"));
        assert!(!EndSequenceDetector::new("").enabled());
        assert!(!EndSequenceDetector::new("").feed(b"M30\r\n"));
        assert_eq!(eot_after_end(b"\x04\r\n", false), Some((3, true)));
        assert_eq!(eot_after_end(b"\r\x04", false), Some((2, false)));
        assert_eq!(eot_after_end(b"\x04\r", true), Some((2, true)));
        assert_eq!(eot_after_end(b"X", false), None);
        let mut lf = profile();
        lf.transfer.line_ending = LineEnding::Lf;
        lf.receive.strip_null_bytes = false;
        assert_eq!(normalize_received(b"A\r\nB\x00\rC", &lf), b"A\nB\x00\nC");
    }
}
