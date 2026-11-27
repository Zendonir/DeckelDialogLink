//! Program transfer and NC drip-feed ("Nachladebetrieb") over LSV2.
//!
//! Safety boundary: in drip-feed mode the CNC requests data by acknowledging each
//! telegram while it executes the program. DeckelLink only *delivers* data; the
//! program is selected and started exclusively by the operator at the CNC.
//! Stopping the data flow does not stop the machine - the operator must stop it
//! at the control.

use crate::clock::{CancelToken, Clock};
use crate::error::UserFacingError;
use crate::model::Profile;
use crate::nc::checks::split_lines;
use crate::nc::TransformResult;
use crate::storage::sha256_hex;
use crate::transfer::lsv2::{ms, protocol_error, EventKind, LinkError, Lsv2Link};
use crate::transfer::sender::POLL_INTERVAL_S;
use crate::transfer::{
    EndReason, Event, EventSink, ProgressTracker, ReceiveProgress, ReceiveResult, ReceiveState, SendResult, SendState,
};
use crate::transport::{describe, ErrorKind, Transport, TransportError};

/// Split the transfer data into telegram texts: `(text, end_offset)` pairs.
pub fn build_telegrams(payload: &TransformResult, profile: &Profile) -> Result<Vec<(Vec<u8>, usize)>, String> {
    let p = &profile.protocol;
    let per = usize::try_from(p.lines_per_telegram).unwrap_or(1).max(1);
    let max_len = usize::try_from(p.max_telegram_length).unwrap_or(usize::MAX);
    let mut telegrams = Vec::new();
    let mut start = 0usize;
    for group in payload.line_end_offsets.chunks(per) {
        let Some(&end) = group.last() else { continue };
        let mut text = payload.data.get(start..end).unwrap_or_default().to_vec();
        if !p.include_line_ending {
            text = split_lines(&text).into_iter().flat_map(|line| line.content).collect();
        }
        if text.len() > max_len {
            let line = payload.line_for_offset(start + 1);
            return Err(format!("Telegramm ab Zeile {line} ist {} Zeichen lang (Profilgrenze {max_len}).", text.len()));
        }
        telegrams.push((text, end));
        start = end;
    }
    Ok(telegrams)
}

fn template(text: &str, name: &str) -> Result<Vec<u8>, String> {
    let filled = text.replace("{name}", name);
    if filled.is_ascii() {
        Ok(filled.into_bytes())
    } else {
        Err(format!("Telegramm '{filled}' enthält Zeichen außerhalb von 7-Bit-ASCII."))
    }
}

/// Sends a program (memory transfer) or feeds it on demand (drip-feed).
pub struct Lsv2ProgramSender<'a> {
    transport: &'a mut dyn Transport,
    profile: &'a Profile,
    clock: &'a dyn Clock,
    token: &'a CancelToken,
    events: &'a mut dyn EventSink,
    pub drip_feed: bool,
    pub program_name: String,
    pub state: SendState,
    pub telegrams_sent: usize,
    pub repetitions: usize,
}

impl std::fmt::Debug for Lsv2ProgramSender<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lsv2ProgramSender")
            .field("state", &self.state)
            .field("drip_feed", &self.drip_feed)
            .finish_non_exhaustive()
    }
}

impl<'a> Lsv2ProgramSender<'a> {
    pub fn new(
        transport: &'a mut dyn Transport,
        profile: &'a Profile,
        clock: &'a dyn Clock,
        token: &'a CancelToken,
        events: &'a mut dyn EventSink,
        drip_feed: bool,
    ) -> Self {
        Self {
            transport,
            profile,
            clock,
            token,
            events,
            drip_feed,
            program_name: "PROGRAMM".into(),
            state: SendState::Ready,
            telegrams_sent: 0,
            repetitions: 0,
        }
    }

    fn ack_timeout(&self) -> Option<f64> {
        let p = &self.profile.protocol;
        if self.drip_feed {
            (p.drip_feed_ack_timeout_ms > 0).then(|| ms(p.drip_feed_ack_timeout_ms))
        } else {
            Some(ms(p.ack_timeout_ms))
        }
    }

    /// Send `payload`; never panics, every failure ends in a `Failed` result with a German description.
    pub fn send(&mut self, payload: &TransformResult) -> SendResult {
        let started_at = self.clock.now();
        let t0 = self.clock.monotonic();
        let operation = if self.drip_feed { "Nachladebetrieb (LSV2)" } else { "Senden (LSV2)" };
        let mut tracker = ProgressTracker::new(payload);
        let ack_timeout = self.ack_timeout();
        let drip = self.drip_feed;
        let name = self.program_name.clone();
        let profile = self.profile;
        let clock = self.clock;
        let token = self.token.clone();
        let mut link = Lsv2Link::new(&mut *self.transport, &profile.protocol, clock, token.clone(), &mut *self.events);
        let outcome = run_sender(&mut link, profile, clock, &token, payload, &mut tracker, ack_timeout, drip, &name);
        let mut error: Option<UserFacingError> = None;
        let state = match outcome {
            Ok(()) => SendState::Completed,
            Err(LinkError::Cancelled) => {
                if link.established && profile.protocol.send_eot_on_cancel && link.transport().is_open() {
                    // allow this final write although the user cancelled
                    link.set_token(CancelToken::new());
                    let _ = link.finish();
                }
                SendState::Cancelled
            }
            Err(LinkError::Failed(err)) => {
                error = Some(describe(&err, operation, &profile.id));
                SendState::Failed
            }
        };
        link.emit(Event::SendState(state));
        self.telegrams_sent = link.telegrams_sent;
        self.repetitions = link.repetitions;
        drop(link);
        self.state = state;
        self.transport.close();
        SendResult {
            state,
            bytes_total: payload.data.len(),
            bytes_sent: tracker.bytes_done,
            started_at,
            duration_s: self.clock.monotonic() - t0,
            sha256: sha256_hex(&payload.data),
            profile_id: profile.id.clone(),
            port: self.transport.name(),
            simulated: self.transport.is_simulated(),
            error,
            port_left_open: false,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_sender(
    link: &mut Lsv2Link<'_>,
    profile: &Profile,
    clock: &dyn Clock,
    token: &CancelToken,
    payload: &TransformResult,
    tracker: &mut ProgressTracker<'_>,
    ack_timeout: Option<f64>,
    drip_feed: bool,
    program_name: &str,
) -> Result<(), LinkError> {
    let port = link.transport_name();
    let telegrams =
        build_telegrams(payload, profile).map_err(|m| protocol_error(ErrorKind::Lsv2Protocol, m, port.clone()))?;
    if token.is_cancelled() {
        return Err(LinkError::Cancelled);
    }
    link.emit(Event::SendState(SendState::OpeningPort));
    if !link.transport().is_open() {
        link.transport().open()?;
    }
    let status = link.transport().modem_status();
    link.emit(Event::Modem(status));
    link.emit(Event::SendState(SendState::WaitingForHandshake));
    wait_lines(link, profile, clock, token)?;
    link.establish()?;
    for text in &profile.protocol.start_telegrams {
        let bytes =
            template(text, program_name).map_err(|m| protocol_error(ErrorKind::Lsv2Protocol, m, port.clone()))?;
        link.send_telegram(&bytes, ack_timeout)?;
    }
    link.emit(Event::SendState(if drip_feed { SendState::DripFeeding } else { SendState::Sending }));
    tracker.start(clock.monotonic());
    link.emit(Event::Progress(tracker.snapshot(clock.monotonic())));
    for (text, end) in &telegrams {
        link.send_telegram(text, ack_timeout)?;
        let progress = tracker.advance(end.saturating_sub(tracker.bytes_done), clock.monotonic());
        link.emit(Event::Progress(progress));
    }
    if tracker.bytes_done < payload.data.len() {
        // A trailing EOT of the file is replaced by the link-level EOT.
        let rest = payload.data.len() - tracker.bytes_done;
        tracker.advance(rest, clock.monotonic());
    }
    for text in &profile.protocol.end_telegrams {
        let bytes =
            template(text, program_name).map_err(|m| protocol_error(ErrorKind::Lsv2Protocol, m, port.clone()))?;
        link.send_telegram(&bytes, ack_timeout)?;
    }
    link.emit(Event::SendState(SendState::Finishing));
    link.finish()?;
    let status = link.transport().modem_status();
    link.emit(Event::Modem(status));
    Ok(())
}

fn wait_lines(
    link: &mut Lsv2Link<'_>,
    profile: &Profile,
    clock: &dyn Clock,
    token: &CancelToken,
) -> Result<(), LinkError> {
    let serial = &profile.serial;
    if !(serial.require_cts_before_send || serial.require_dsr_before_send) {
        return Ok(());
    }
    let deadline = clock.monotonic() + ms(serial.handshake_timeout_ms);
    loop {
        let status = link.transport().modem_status();
        link.emit(Event::Modem(status));
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
        if clock.monotonic() >= deadline {
            let mut err = TransportError::new(
                ErrorKind::HandshakeTimeout,
                format!("{} nicht aktiv", missing.join("/")),
                link.transport_name(),
            );
            err.missing = missing;
            return Err(LinkError::Failed(err));
        }
        if clock.sleep(POLL_INTERVAL_S, Some(token)) {
            return Err(LinkError::Cancelled);
        }
    }
}

/// Receives a program that the CNC sends via LSV2.
pub struct Lsv2ProgramReceiver<'a> {
    transport: &'a mut dyn Transport,
    profile: &'a Profile,
    clock: &'a dyn Clock,
    token: &'a CancelToken,
    events: &'a mut dyn EventSink,
    pub repetitions: usize,
}

impl std::fmt::Debug for Lsv2ProgramReceiver<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lsv2ProgramReceiver").finish_non_exhaustive()
    }
}

struct ReceiveOutcome {
    end_reason: EndReason,
    state: ReceiveState,
    error: Option<UserFacingError>,
    first_at: Option<chrono::DateTime<chrono::FixedOffset>>,
    last_at: Option<chrono::DateTime<chrono::FixedOffset>>,
}

impl<'a> Lsv2ProgramReceiver<'a> {
    pub fn new(
        transport: &'a mut dyn Transport,
        profile: &'a Profile,
        clock: &'a dyn Clock,
        token: &'a CancelToken,
        events: &'a mut dyn EventSink,
    ) -> Self {
        Self { transport, profile, clock, token, events, repetitions: 0 }
    }

    pub fn receive(&mut self) -> ReceiveResult {
        let started_at = self.clock.now();
        let t0 = self.clock.monotonic();
        let profile = self.profile;
        let clock = self.clock;
        let mut buffer: Vec<u8> = Vec::new();
        let mut link =
            Lsv2Link::new(&mut *self.transport, &profile.protocol, clock, self.token.clone(), &mut *self.events);
        let mut outcome = ReceiveOutcome {
            end_reason: EndReason::Error,
            state: ReceiveState::Failed,
            error: None,
            first_at: None,
            last_at: None,
        };
        match run_receiver(&mut link, profile, clock, t0, &mut buffer, &mut outcome) {
            Ok(()) => {}
            Err(LinkError::Cancelled) => {
                outcome.end_reason = EndReason::Cancelled;
                outcome.state = ReceiveState::Cancelled;
            }
            Err(LinkError::Failed(err)) => {
                outcome.end_reason = EndReason::Error;
                outcome.state = ReceiveState::Failed;
                outcome.error = Some(describe(&err, "Empfangen (LSV2)", &profile.id));
            }
        }
        link.emit(Event::ReceiveState(outcome.state));
        self.repetitions = link.repetitions;
        drop(link);
        self.transport.close();
        ReceiveResult {
            state: outcome.state,
            raw: buffer,
            end_reason: outcome.end_reason,
            started_at,
            duration_s: self.clock.monotonic() - t0,
            profile_id: profile.id.clone(),
            port: self.transport.name(),
            simulated: self.transport.is_simulated(),
            first_byte_at: outcome.first_at,
            last_byte_at: outcome.last_at,
            error: outcome.error,
        }
    }
}

fn run_receiver(
    link: &mut Lsv2Link<'_>,
    profile: &Profile,
    clock: &dyn Clock,
    t0: f64,
    buffer: &mut Vec<u8>,
    outcome: &mut ReceiveOutcome,
) -> Result<(), LinkError> {
    let settings = &profile.receive;
    let ending = profile.transfer.line_ending.sequence();
    let max_bytes = usize::try_from(settings.max_receive_bytes).unwrap_or(usize::MAX);
    link.emit(Event::ReceiveState(ReceiveState::OpeningPort));
    if !link.transport().is_open() {
        link.transport().open()?;
    }
    let status = link.transport().modem_status();
    link.emit(Event::Modem(status));
    link.emit(Event::ReceiveState(ReceiveState::Listening));
    let first_timeout = (settings.first_byte_timeout_ms > 0).then(|| ms(settings.first_byte_timeout_ms));
    if !link.accept(first_timeout)? {
        outcome.end_reason = EndReason::NoData;
        outcome.state = ReceiveState::TimedOut;
        link.emit(Event::ReceiveState(ReceiveState::Finalizing));
        return Ok(());
    }
    outcome.first_at = Some(clock.now());
    link.emit(Event::ReceiveState(ReceiveState::Receiving));
    loop {
        let Some(event) = link.receive_event(Some(ms(settings.inactivity_timeout_ms)))? else {
            outcome.end_reason = EndReason::Inactivity;
            outcome.state = ReceiveState::TimedOut;
            break;
        };
        if event.kind == EventKind::Eot {
            outcome.end_reason = EndReason::Eot;
            outcome.state = ReceiveState::Completed;
            break;
        }
        let mut text = event.text;
        if !profile.protocol.include_line_ending {
            text.extend_from_slice(ending);
        }
        outcome.last_at = Some(clock.now());
        if max_bytes > 0 && buffer.len() + text.len() > max_bytes {
            outcome.end_reason = EndReason::MaxSize;
            outcome.state = ReceiveState::Failed;
            outcome.error = Some(UserFacingError {
                title: "Maximale Empfangsgröße überschritten - Empfang gestoppt.".into(),
                causes: vec![format!("Grenze laut Profil: {} Bytes.", settings.max_receive_bytes)],
                technical_details: String::new(),
                operation: "Empfangen (LSV2)".into(),
            });
            return Ok(());
        }
        buffer.extend_from_slice(&text);
        link.emit(Event::ReceiveProgress(ReceiveProgress {
            state: ReceiveState::Receiving,
            bytes_received: buffer.len(),
            elapsed_s: clock.monotonic() - t0,
            idle_s: 0.0,
        }));
    }
    link.emit(Event::ReceiveState(ReceiveState::Finalizing));
    Ok(())
}
