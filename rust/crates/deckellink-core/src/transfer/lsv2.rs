//! LSV2 link layer (DIN 66019 / ISO 1745 basic mode) over a transport.
//!
//! Implemented procedure (public standard; its exact use by the DIALOG 11 is NOT verified):
//! * Link establishment: the sender transmits `ENQ`; the receiver answers `DLE 0` when ready.
//! * Telegram: `[DLE] STX <text> [DLE] ETX <BCC>`; in transparent mode every DLE in the text is doubled.
//! * Acknowledgements alternate `DLE 1` / `DLE 0`; `NAK` requests a repetition; `DLE ;` (WACK) = wait.
//! * On a missing acknowledgement the sender transmits `ENQ`; the receiver repeats its last acknowledgement.
//! * `EOT` ends the link; `DLE <` (RVI) from the receiver is treated as a request to stop.
//!
//! The CNC paces the data stream through its acknowledgements (drip-feed). DeckelLink
//! never starts program execution.

use std::collections::VecDeque;

use crate::clock::{CancelToken, Clock};
use crate::model::{BccMode, ProtocolSettings};
use crate::transfer::{Direction, Event, EventSink, TrafficEvent};
use crate::transport::{ErrorKind, Transport, TransportError};

pub const STX: u8 = 0x02;
pub const ETX: u8 = 0x03;
pub const EOT: u8 = 0x04;
pub const ENQ: u8 = 0x05;
pub const DLE: u8 = 0x10;
pub const NAK: u8 = 0x15;
pub const ACK0: [u8; 2] = [DLE, 0x30];
pub const ACK1: [u8; 2] = [DLE, 0x31];
pub const WACK: [u8; 2] = [DLE, 0x3B];
pub const RVI: [u8; 2] = [DLE, 0x3C];

/// Error of the LSV2 layer: a transport error, or the user cancelled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkError {
    Cancelled,
    Failed(TransportError),
}

impl From<TransportError> for LinkError {
    fn from(err: TransportError) -> Self {
        LinkError::Failed(err)
    }
}

pub fn protocol_error(kind: ErrorKind, message: impl Into<String>, port: impl Into<String>) -> LinkError {
    LinkError::Failed(TransportError::new(kind, message, port))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Enq,
    Eot,
    Nak,
    Ack0,
    Ack1,
    Wack,
    Rvi,
    Telegram,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkEvent {
    pub kind: EventKind,
    pub text: Vec<u8>,
    pub bcc_ok: bool,
}

impl LinkEvent {
    fn simple(kind: EventKind) -> Self {
        Self { kind, text: Vec::new(), bcc_ok: true }
    }
}

pub fn xor_bytes(data: &[u8]) -> u8 {
    data.iter().fold(0, |acc, b| acc ^ b)
}

/// XOR block check: over text + ETX, or over all transmitted bytes after STX.
pub fn compute_bcc(text: &[u8], transmitted_after_stx: &[u8], mode: BccMode) -> u8 {
    match mode {
        BccMode::TextEtx => xor_bytes(text) ^ ETX,
        BccMode::Transmitted | BccMode::None => xor_bytes(transmitted_after_stx),
    }
}

/// Frame `text` as a telegram including BCC.
pub fn encode_telegram(text: &[u8], settings: &ProtocolSettings) -> Result<Vec<u8>, String> {
    let (mut frame, after_stx) = if settings.transparent {
        let mut body = Vec::with_capacity(text.len() + 4);
        for &b in text {
            body.push(b);
            if b == DLE {
                body.push(DLE);
            }
        }
        let mut after = body.clone();
        after.extend_from_slice(&[DLE, ETX]);
        let mut frame = vec![DLE, STX];
        frame.extend_from_slice(&after);
        (frame, after)
    } else {
        if text.iter().any(|b| matches!(*b, STX | ETX | DLE)) {
            return Err("Telegrammtext enthält Steuerzeichen (nur im Transparentmodus zulässig).".into());
        }
        let mut after = text.to_vec();
        after.push(ETX);
        let mut frame = vec![STX];
        frame.extend_from_slice(&after);
        (frame, after)
    };
    if settings.bcc_mode != BccMode::None {
        frame.push(compute_bcc(text, &after_stx, settings.bcc_mode));
    }
    Ok(frame)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DecoderState {
    Idle,
    IdleDle,
    Text,
    TextDle,
    Bcc,
}

/// Incremental decoder turning received bytes into [`LinkEvent`]s.
#[derive(Debug, Clone)]
pub struct FrameDecoder {
    transparent: bool,
    bcc_mode: BccMode,
    max_text: usize,
    pub ignored_bytes: usize,
    state: DecoderState,
    text: Vec<u8>,
    raw: Vec<u8>,
}

impl FrameDecoder {
    pub fn new(settings: &ProtocolSettings, max_text: usize) -> Self {
        Self {
            transparent: settings.transparent,
            bcc_mode: settings.bcc_mode,
            max_text,
            ignored_bytes: 0,
            state: DecoderState::Idle,
            text: Vec::new(),
            raw: Vec::new(),
        }
    }

    pub fn feed(&mut self, data: &[u8]) -> Vec<LinkEvent> {
        data.iter().filter_map(|b| self.step(*b)).collect()
    }

    fn begin_text(&mut self) {
        self.state = DecoderState::Text;
        self.text.clear();
        self.raw.clear();
    }

    fn end_text(&mut self) -> Option<LinkEvent> {
        if self.bcc_mode == BccMode::None {
            self.state = DecoderState::Idle;
            return Some(LinkEvent { kind: EventKind::Telegram, text: std::mem::take(&mut self.text), bcc_ok: true });
        }
        self.state = DecoderState::Bcc;
        None
    }

    fn step(&mut self, byte: u8) -> Option<LinkEvent> {
        match self.state {
            DecoderState::Idle => {
                match byte {
                    ENQ => return Some(LinkEvent::simple(EventKind::Enq)),
                    EOT => return Some(LinkEvent::simple(EventKind::Eot)),
                    NAK => return Some(LinkEvent::simple(EventKind::Nak)),
                    DLE => self.state = DecoderState::IdleDle,
                    STX if !self.transparent => self.begin_text(),
                    _ => self.ignored_bytes += 1,
                }
                None
            }
            DecoderState::IdleDle => {
                self.state = DecoderState::Idle;
                match byte {
                    0x30 => Some(LinkEvent::simple(EventKind::Ack0)),
                    0x31 => Some(LinkEvent::simple(EventKind::Ack1)),
                    0x3B => Some(LinkEvent::simple(EventKind::Wack)),
                    0x3C => Some(LinkEvent::simple(EventKind::Rvi)),
                    STX if self.transparent => {
                        self.begin_text();
                        None
                    }
                    _ => {
                        self.ignored_bytes += 2;
                        None
                    }
                }
            }
            DecoderState::Text => {
                self.raw.push(byte);
                if self.transparent && byte == DLE {
                    self.state = DecoderState::TextDle;
                    return None;
                }
                if !self.transparent && byte == ETX {
                    return self.end_text();
                }
                self.text.push(byte);
                if self.text.len() > self.max_text {
                    self.state = DecoderState::Idle;
                    return Some(LinkEvent {
                        kind: EventKind::Telegram,
                        text: std::mem::take(&mut self.text),
                        bcc_ok: false,
                    });
                }
                None
            }
            DecoderState::TextDle => {
                self.raw.push(byte);
                match byte {
                    DLE => {
                        self.text.push(DLE);
                        self.state = DecoderState::Text;
                        None
                    }
                    ETX => self.end_text(),
                    STX => {
                        // resynchronisation: a new telegram starts
                        self.begin_text();
                        None
                    }
                    _ => {
                        self.state = DecoderState::Idle;
                        Some(LinkEvent {
                            kind: EventKind::Telegram,
                            text: std::mem::take(&mut self.text),
                            bcc_ok: false,
                        })
                    }
                }
            }
            DecoderState::Bcc => {
                self.state = DecoderState::Idle;
                let expected = compute_bcc(&self.text, &self.raw, self.bcc_mode);
                Some(LinkEvent {
                    kind: EventKind::Telegram,
                    text: std::mem::take(&mut self.text),
                    bcc_ok: byte == expected,
                })
            }
        }
    }
}

#[allow(clippy::cast_precision_loss)]
pub(crate) fn ms(value: u64) -> f64 {
    value as f64 / 1000.0
}

/// One end of an LSV2 link on top of a [`Transport`].
pub struct Lsv2Link<'a> {
    transport: &'a mut dyn Transport,
    settings: ProtocolSettings,
    clock: &'a dyn Clock,
    token: CancelToken,
    events: &'a mut dyn EventSink,
    decoder: FrameDecoder,
    queue: VecDeque<LinkEvent>,
    next_ack: [u8; 2],
    last_ack: [u8; 2],
    pub read_size: usize,
    pub established: bool,
    pub telegrams_sent: usize,
    pub repetitions: usize,
}

impl std::fmt::Debug for Lsv2Link<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lsv2Link")
            .field("established", &self.established)
            .field("telegrams_sent", &self.telegrams_sent)
            .finish_non_exhaustive()
    }
}

impl<'a> Lsv2Link<'a> {
    pub fn new(
        transport: &'a mut dyn Transport,
        settings: &ProtocolSettings,
        clock: &'a dyn Clock,
        token: CancelToken,
        events: &'a mut dyn EventSink,
    ) -> Self {
        let max_text = usize::try_from(settings.max_telegram_length).unwrap_or(1024).saturating_mul(2).max(1024);
        Self {
            transport,
            settings: settings.clone(),
            clock,
            token,
            events,
            decoder: FrameDecoder::new(settings, max_text),
            queue: VecDeque::new(),
            next_ack: ACK1,
            last_ack: ACK0,
            read_size: 64,
            established: false,
            telegrams_sent: 0,
            repetitions: 0,
        }
    }

    pub fn transport_name(&self) -> String {
        self.transport.name()
    }

    pub fn transport(&mut self) -> &mut dyn Transport {
        &mut *self.transport
    }

    /// Report a transfer event (state, progress, modem lines) through the link's sink.
    pub fn emit(&mut self, event: Event) {
        self.events.emit(event);
    }

    /// Replace the cancellation token (e.g. to allow a final EOT after the user cancelled).
    pub fn set_token(&mut self, token: CancelToken) {
        self.token = token;
    }

    fn check(&self) -> Result<(), LinkError> {
        if self.token.is_cancelled() {
            Err(LinkError::Cancelled)
        } else {
            Ok(())
        }
    }

    pub fn write(&mut self, data: &[u8]) -> Result<(), LinkError> {
        let mut pos = 0usize;
        let mut stall_since: Option<f64> = None;
        let timeout = ms(self.settings.ack_timeout_ms);
        while pos < data.len() {
            self.check()?;
            let rest = data.get(pos..).unwrap_or_default();
            let written = self.transport.write(rest, &self.token)?.min(rest.len());
            if written > 0 {
                self.events.emit(Event::Traffic(TrafficEvent {
                    direction: Direction::Tx,
                    timestamp: self.clock.now(),
                    data: rest.get(..written).unwrap_or_default().to_vec(),
                }));
                pos += written;
                stall_since = None;
                continue;
            }
            let now = self.clock.monotonic();
            match stall_since {
                None => stall_since = Some(now),
                Some(since) if now - since >= timeout => {
                    return Err(protocol_error(
                        ErrorKind::Timeout,
                        "Schreibvorgang ohne Fortschritt",
                        self.transport.name(),
                    ));
                }
                Some(_) => {}
            }
            if self.clock.sleep(0.01, Some(&self.token)) {
                return Err(LinkError::Cancelled);
            }
        }
        Ok(())
    }

    /// Next decoded event, or `None` after `timeout_s` (`None` = wait forever).
    pub fn wait_event(&mut self, timeout_s: Option<f64>) -> Result<Option<LinkEvent>, LinkError> {
        let deadline = timeout_s.map(|t| self.clock.monotonic() + t);
        loop {
            if let Some(event) = self.queue.pop_front() {
                return Ok(Some(event));
            }
            self.check()?;
            if deadline.is_some_and(|d| self.clock.monotonic() >= d) {
                return Ok(None);
            }
            let data = self.transport.read(self.read_size, &self.token)?;
            self.check()?;
            if !data.is_empty() {
                self.events.emit(Event::Traffic(TrafficEvent {
                    direction: Direction::Rx,
                    timestamp: self.clock.now(),
                    data: data.clone(),
                }));
                let decoded = self.decoder.feed(&data);
                self.queue.extend(decoded);
            }
        }
    }

    // -- sender role ------------------------------------------------------
    /// Send ENQ until the CNC answers ACK0 (or `link_timeout_ms` expires).
    pub fn establish(&mut self) -> Result<(), LinkError> {
        let deadline = self.clock.monotonic() + ms(self.settings.link_timeout_ms);
        loop {
            self.write(&[ENQ])?;
            let interval_end = self.clock.monotonic() + ms(self.settings.enq_interval_ms);
            loop {
                let remaining = (interval_end - self.clock.monotonic()).max(0.0);
                let Some(event) = self.wait_event(Some(remaining))? else { break };
                match event.kind {
                    EventKind::Ack0 => {
                        self.established = true;
                        self.next_ack = ACK1;
                        return Ok(());
                    }
                    EventKind::Eot => {
                        return Err(protocol_error(
                            ErrorKind::CncAbort,
                            "Die CNC hat den Verbindungsaufbau mit EOT abgelehnt.",
                            self.transport.name(),
                        ));
                    }
                    EventKind::Nak => break,
                    _ => {}
                }
            }
            if self.clock.monotonic() >= deadline {
                return Err(protocol_error(
                    ErrorKind::Lsv2LinkTimeout,
                    format!(
                        "Keine Empfangsbereitschaft (DLE 0) der CNC innerhalb {} ms",
                        self.settings.link_timeout_ms
                    ),
                    self.transport.name(),
                ));
            }
            let remaining = interval_end - self.clock.monotonic();
            if remaining > 0.0 && self.clock.sleep(remaining, Some(&self.token)) {
                return Err(LinkError::Cancelled);
            }
        }
    }

    /// Send one telegram and wait for the matching acknowledgement (`None` = wait forever).
    pub fn send_telegram(&mut self, text: &[u8], ack_timeout_s: Option<f64>) -> Result<(), LinkError> {
        let name = self.transport.name();
        let frame = encode_telegram(text, &self.settings)
            .map_err(|m| protocol_error(ErrorKind::Lsv2Protocol, m, name.clone()))?;
        let expected = self.next_ack;
        let previous = if expected == ACK1 { ACK0 } else { ACK1 };
        let expected_kind = if expected == ACK1 { EventKind::Ack1 } else { EventKind::Ack0 };
        let previous_kind = if previous == ACK0 { EventKind::Ack0 } else { EventKind::Ack1 };
        let max_retries = usize::try_from(self.settings.max_retries).unwrap_or(usize::MAX);
        let mut errors = 0usize;
        self.write(&frame)?;
        loop {
            let Some(event) = self.wait_event(ack_timeout_s)? else {
                errors += 1;
                if errors > max_retries {
                    return Err(protocol_error(
                        ErrorKind::Lsv2AckTimeout,
                        format!("Keine Quittung der CNC für Telegramm {}", self.telegrams_sent + 1),
                        name,
                    ));
                }
                self.write(&[ENQ])?;
                continue;
            };
            let kind = event.kind;
            if kind == expected_kind {
                self.next_ack = previous;
                self.telegrams_sent += 1;
                return Ok(());
            }
            match kind {
                EventKind::Wack => {
                    if self.clock.sleep(ms(self.settings.wack_poll_ms), Some(&self.token)) {
                        return Err(LinkError::Cancelled);
                    }
                    self.write(&[ENQ])?;
                }
                EventKind::Nak => self.repeat(&frame, &mut errors, max_retries)?,
                _ if kind == previous_kind => self.repeat(&frame, &mut errors, max_retries)?,
                EventKind::Eot => {
                    return Err(protocol_error(
                        ErrorKind::CncAbort,
                        "Die CNC hat die Übertragung mit EOT beendet.",
                        name,
                    ));
                }
                EventKind::Rvi => {
                    return Err(protocol_error(
                        ErrorKind::CncAbort,
                        "Die CNC hat eine Unterbrechung (RVI) angefordert.",
                        name,
                    ));
                }
                _ => {}
            }
        }
    }

    fn repeat(&mut self, frame: &[u8], errors: &mut usize, max_retries: usize) -> Result<(), LinkError> {
        *errors += 1;
        self.repetitions += 1;
        if *errors > max_retries {
            return Err(protocol_error(
                ErrorKind::Lsv2RetryExceeded,
                format!(
                    "Telegramm {} nach {} Wiederholungen nicht quittiert",
                    self.telegrams_sent + 1,
                    self.settings.max_retries
                ),
                self.transport.name(),
            ));
        }
        self.write(frame)
    }

    pub fn finish(&mut self) -> Result<(), LinkError> {
        self.write(&[EOT])?;
        self.established = false;
        Ok(())
    }

    // -- receiver role ----------------------------------------------------
    /// Wait for ENQ from the CNC and confirm readiness with ACK0.
    pub fn accept(&mut self, timeout_s: Option<f64>) -> Result<bool, LinkError> {
        loop {
            let Some(event) = self.wait_event(timeout_s)? else { return Ok(false) };
            if event.kind == EventKind::Enq {
                self.write(&ACK0)?;
                self.last_ack = ACK0;
                self.next_ack = ACK1;
                self.established = true;
                return Ok(true);
            }
        }
    }

    /// Receiver step: acknowledges telegrams and answers ENQ polls. Returns telegrams, EOT or `None`.
    pub fn receive_event(&mut self, timeout_s: Option<f64>) -> Result<Option<LinkEvent>, LinkError> {
        loop {
            let Some(event) = self.wait_event(timeout_s)? else { return Ok(None) };
            match event.kind {
                EventKind::Telegram => {
                    if !event.bcc_ok {
                        self.repetitions += 1;
                        self.write(&[NAK])?;
                        continue;
                    }
                    let ack = self.next_ack;
                    self.write(&ack)?;
                    self.last_ack = ack;
                    self.next_ack = if ack == ACK1 { ACK0 } else { ACK1 };
                    return Ok(Some(event));
                }
                EventKind::Enq => {
                    let last = self.last_ack;
                    self.write(&last)?;
                }
                EventKind::Eot => {
                    self.established = false;
                    return Ok(Some(event));
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(transparent: bool, bcc: BccMode) -> ProtocolSettings {
        ProtocolSettings { transparent, bcc_mode: bcc, ..ProtocolSettings::default() }
    }

    #[test]
    fn encode_and_decode_roundtrip() {
        for transparent in [true, false] {
            for bcc in [BccMode::TextEtx, BccMode::Transmitted, BccMode::None] {
                let s = settings(transparent, bcc);
                let text: &[u8] = if transparent { b"N10 \x10G0\r\n" } else { b"N10 G0\r\n" };
                let frame = encode_telegram(text, &s).unwrap();
                let mut decoder = FrameDecoder::new(&s, 1024);
                let events = decoder.feed(&frame);
                assert_eq!(events.len(), 1, "{transparent} {bcc:?}");
                assert_eq!(events[0].text, text);
                assert!(events[0].bcc_ok);
            }
        }
        assert!(encode_telegram(b"\x02", &settings(false, BccMode::TextEtx)).is_err());
        let frame = encode_telegram(b"AB", &settings(true, BccMode::TextEtx)).unwrap();
        assert_eq!(frame, vec![DLE, STX, b'A', b'B', DLE, ETX, b'A' ^ b'B' ^ ETX]);
    }

    #[test]
    fn decoder_control_characters_and_errors() {
        let s = settings(true, BccMode::TextEtx);
        let mut d = FrameDecoder::new(&s, 4);
        let kinds: Vec<_> = d
            .feed(&[ENQ, EOT, NAK, DLE, 0x30, DLE, 0x31, DLE, 0x3B, DLE, 0x3C, 0x00, DLE, 0x99])
            .into_iter()
            .map(|e| e.kind)
            .collect();
        assert_eq!(
            kinds,
            vec![
                EventKind::Enq,
                EventKind::Eot,
                EventKind::Nak,
                EventKind::Ack0,
                EventKind::Ack1,
                EventKind::Wack,
                EventKind::Rvi
            ]
        );
        assert_eq!(d.ignored_bytes, 3);
        let mut frame = encode_telegram(b"AB", &s).unwrap();
        if let Some(last) = frame.last_mut() {
            *last ^= 1;
        }
        assert!(!d.feed(&frame)[0].bcc_ok);
        assert!(!d.feed(&[DLE, STX, b'A', b'B', b'C', b'D', b'E'])[0].bcc_ok); // too long
        assert!(!d.feed(&[DLE, STX, b'A', DLE, b'X'])[0].bcc_ok);
        let events = d.feed(&[DLE, STX, b'A', DLE, STX, b'B', DLE, ETX, b'B' ^ ETX]);
        assert_eq!(events[0].text, b"B");
    }
}
