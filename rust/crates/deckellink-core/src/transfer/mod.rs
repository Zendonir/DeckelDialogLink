//! Sending and receiving (plain protocol). LSV2 follows in a later stage of the Rust port.

pub mod lsv2;
pub mod lsv2_transfer;
pub mod receiver;
pub mod sender;

use chrono::{DateTime, FixedOffset};

use crate::error::UserFacingError;
use crate::nc::TransformResult;
use crate::transport::ModemStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendState {
    Idle,
    Ready,
    OpeningPort,
    WaitingForHandshake,
    Sending,
    DripFeeding,
    Finishing,
    Completed,
    Cancelled,
    Failed,
}

impl SendState {
    pub fn label(self) -> &'static str {
        match self {
            SendState::Idle => "Bereit - keine Datei",
            SendState::Ready => "Bereit zum Senden",
            SendState::OpeningPort => "Port wird geöffnet",
            SendState::WaitingForHandshake => "Warte auf Freigabe (Handshake)",
            SendState::Sending => "Sende Daten",
            SendState::DripFeeding => "Nachladebetrieb - Daten werden nach Quittung der CNC geliefert",
            SendState::Finishing => "Übertragung wird abgeschlossen",
            SendState::Completed => "Übertragung abgeschlossen",
            SendState::Cancelled => "Übertragung abgebrochen",
            SendState::Failed => "Übertragung fehlgeschlagen",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, SendState::Completed | SendState::Cancelled | SendState::Failed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiveState {
    Idle,
    OpeningPort,
    Listening,
    Receiving,
    Finalizing,
    Completed,
    Cancelled,
    TimedOut,
    Failed,
}

impl ReceiveState {
    pub fn label(self) -> &'static str {
        match self {
            ReceiveState::Idle => "Bereit",
            ReceiveState::OpeningPort => "Port wird geöffnet",
            ReceiveState::Listening => "Empfangsbereit - warte auf Daten der CNC",
            ReceiveState::Receiving => "Empfange Daten",
            ReceiveState::Finalizing => "Empfang wird abgeschlossen",
            ReceiveState::Completed => "Empfang abgeschlossen",
            ReceiveState::Cancelled => "Empfang abgebrochen",
            ReceiveState::TimedOut => "Zeitüberschreitung beim Empfang",
            ReceiveState::Failed => "Empfang fehlgeschlagen",
        }
    }
}

/// Why a receive operation ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    EndSequence,
    Inactivity,
    MaxSize,
    NoData,
    Cancelled,
    Error,
    Eot,
}

impl EndReason {
    pub fn label(self) -> &'static str {
        match self {
            EndReason::EndSequence => "Endsequenz erkannt",
            EndReason::Inactivity => "Inaktivitätszeit abgelaufen",
            EndReason::MaxSize => "Maximale Empfangsgröße erreicht",
            EndReason::NoData => "Keine Daten empfangen",
            EndReason::Cancelled => "Durch Benutzer abgebrochen",
            EndReason::Error => "Übertragungsfehler",
            EndReason::Eot => "Übertragungsende (EOT) der CNC",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Tx,
    Rx,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrafficEvent {
    pub direction: Direction,
    pub timestamp: DateTime<FixedOffset>,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TransferProgress {
    pub bytes_done: usize,
    pub bytes_total: usize,
    pub current_line: usize,
    pub total_lines: usize,
    pub elapsed_s: f64,
}

impl TransferProgress {
    pub fn percent(&self) -> f64 {
        if self.bytes_total == 0 {
            return 0.0;
        }
        #[allow(clippy::cast_precision_loss)]
        let value = 100.0 * self.bytes_done as f64 / self.bytes_total as f64;
        value.min(100.0)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReceiveProgress {
    pub state: ReceiveState,
    pub bytes_received: usize,
    pub elapsed_s: f64,
    pub idle_s: f64,
}

/// Events reported by sender/receiver (sent to the GUI over a channel).
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    SendState(SendState),
    ReceiveState(ReceiveState),
    Progress(TransferProgress),
    ReceiveProgress(ReceiveProgress),
    Traffic(TrafficEvent),
    Modem(ModemStatus),
}

/// Receiver of transfer events. Implementations must not block for long.
pub trait EventSink {
    fn emit(&mut self, event: Event);
}

/// Discards all events.
#[derive(Debug, Default)]
pub struct NoEvents;

impl EventSink for NoEvents {
    fn emit(&mut self, _event: Event) {}
}

impl EventSink for Vec<Event> {
    fn emit(&mut self, event: Event) {
        self.push(event);
    }
}

impl EventSink for std::sync::mpsc::Sender<Event> {
    fn emit(&mut self, event: Event) {
        // The receiving side may be gone (window closed) - never fail the transfer for that.
        let _ = self.send(event);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SendResult {
    pub state: SendState,
    pub bytes_total: usize,
    pub bytes_sent: usize,
    pub started_at: DateTime<FixedOffset>,
    pub duration_s: f64,
    pub sha256: String,
    pub profile_id: String,
    pub port: String,
    pub simulated: bool,
    pub error: Option<UserFacingError>,
    pub port_left_open: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReceiveResult {
    pub state: ReceiveState,
    pub raw: Vec<u8>,
    pub end_reason: EndReason,
    pub started_at: DateTime<FixedOffset>,
    pub duration_s: f64,
    pub profile_id: String,
    pub port: String,
    pub simulated: bool,
    pub first_byte_at: Option<DateTime<FixedOffset>>,
    pub last_byte_at: Option<DateTime<FixedOffset>>,
    pub error: Option<UserFacingError>,
}

/// Progress calculation for a payload.
#[derive(Debug)]
pub struct ProgressTracker<'a> {
    payload: &'a TransformResult,
    t0: Option<f64>,
    pub bytes_done: usize,
}

impl<'a> ProgressTracker<'a> {
    pub fn new(payload: &'a TransformResult) -> Self {
        Self { payload, t0: None, bytes_done: 0 }
    }

    pub fn start(&mut self, now: f64) {
        self.t0 = Some(now);
    }

    pub fn advance(&mut self, count: usize, now: f64) -> TransferProgress {
        self.bytes_done += count;
        self.snapshot(now)
    }

    pub fn snapshot(&self, now: f64) -> TransferProgress {
        let total = self.payload.data.len();
        let mut line = if self.bytes_done > 0 { self.payload.line_for_offset(self.bytes_done) } else { 0 };
        if total > 0 && self.bytes_done >= total {
            line = self.payload.line_count;
        }
        TransferProgress {
            bytes_done: self.bytes_done,
            bytes_total: total,
            current_line: line,
            total_lines: self.payload.line_count,
            elapsed_s: self.t0.map_or(0.0, |t0| now - t0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress() {
        let payload = TransformResult {
            data: b"N1\r\nN2\r\n".to_vec(),
            line_count: 2,
            line_end_offsets: vec![4, 8],
            ..TransformResult::default()
        };
        let mut t = ProgressTracker::new(&payload);
        assert_eq!(t.snapshot(0.0).current_line, 0);
        t.start(1.0);
        let p = t.advance(5, 3.0);
        assert_eq!((p.current_line, p.elapsed_s), (2, 2.0));
        assert!((p.percent() - 62.5).abs() < 1e-9);
        assert_eq!(t.advance(3, 3.0).current_line, 2);
        let empty = TransferProgress { bytes_done: 0, bytes_total: 0, current_line: 0, total_lines: 0, elapsed_s: 0.0 };
        assert_eq!(empty.percent(), 0.0);
        assert!(SendState::Completed.is_terminal() && !SendState::Sending.is_terminal());
        assert_eq!(EndReason::Eot.label(), "Übertragungsende (EOT) der CNC");
        assert_eq!(ReceiveState::Listening.label(), "Empfangsbereit - warte auf Daten der CNC");
        assert_eq!(SendState::Sending.label(), "Sende Daten");
    }
}
