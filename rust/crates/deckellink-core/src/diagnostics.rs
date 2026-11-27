//! Diagnostics: serial loopback test (TX bridged to RX) and passive data format detection.
//!
//! Both functions mirror the Python version. Format detection only listens - it never
//! writes to the machine.

use crate::clock::{CancelToken, Clock};
use crate::error::UserFacingError;
use crate::model::{Parity, SerialSettings, StopBits};
use crate::nc::checks::max_value_for_data_bits;
use crate::transfer::{Direction, Event, EventSink, TrafficEvent};
use crate::transport::{describe, ModemStatus, Transport};

// ------------------------------------------------------------------- loopback

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopbackPattern {
    AsciiText,
    ByteRange,
    Repeated,
    VariablePackets,
}

impl LoopbackPattern {
    pub const ALL: [LoopbackPattern; 4] = [
        LoopbackPattern::AsciiText,
        LoopbackPattern::ByteRange,
        LoopbackPattern::Repeated,
        LoopbackPattern::VariablePackets,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LoopbackPattern::AsciiText => "ASCII-Text",
            LoopbackPattern::ByteRange => "Bytefolge (zulässiger Datenbitbereich)",
            LoopbackPattern::Repeated => "Wiederholtes Muster 0x55/0xAA-ähnlich",
            LoopbackPattern::VariablePackets => "Variable Paketgrößen",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopbackVerdict {
    Passed,
    Partial,
    Failed,
}

impl LoopbackVerdict {
    pub fn label(self) -> &'static str {
        match self {
            LoopbackVerdict::Passed => "Bestanden",
            LoopbackVerdict::Partial => "Teilweise bestanden",
            LoopbackVerdict::Failed => "Fehlgeschlagen",
        }
    }
}

pub const FAILURE_HINTS: [&str; 7] = [
    "TX und RX sind am Stecker nicht gebrückt (DB9: Pin 2-3, DB25: Pin 2-3).",
    "Falscher COM-Port ausgewählt.",
    "Der Port ist durch ein anderes Programm belegt.",
    "Falsche Baudrate oder falsches Datenformat (betrifft nur reale Gegenstellen).",
    "Ungeeigneter USB-RS-232-Adapter oder fehlender Treiber.",
    "Handshake blockiert (bei RTS/CTS auch RTS-CTS brücken).",
    "Kein echter RS-232-Pegel (TTL-Adapter mit 3,3/5 V statt ±12 V).",
];

/// Test data as packets (same bytes as the Python version).
pub fn make_pattern(pattern: LoopbackPattern, length: usize, data_bits: u8) -> Vec<Vec<u8>> {
    let length = length.max(1);
    let max_value = max_value_for_data_bits(data_bits);
    let fill = |unit: &[u8]| -> Vec<u8> { unit.iter().copied().cycle().take(length).map(|b| b & max_value).collect() };
    match pattern {
        LoopbackPattern::AsciiText => vec![fill(b"DECKELLINK LOOPBACK 0123456789 ABCDEFGHIJKLMNOPQRSTUVWXYZ\r\n")],
        LoopbackPattern::ByteRange => {
            let modulo = usize::from(max_value) + 1;
            vec![(0..length).map(|i| u8::try_from(i % modulo).unwrap_or(0)).collect()]
        }
        LoopbackPattern::Repeated => vec![fill(&[0x55, 0x2A, 0x55, 0x2A, 0x0F, 0x70])],
        LoopbackPattern::VariablePackets => {
            const SIZES: [usize; 7] = [1, 2, 7, 16, 31, 64, 3];
            let mut packets = Vec::new();
            let mut produced = 0usize;
            for size in SIZES.iter().cycle() {
                if produced >= length {
                    break;
                }
                let size = (*size).min(length - produced);
                packets.push(
                    (0..size)
                        .map(|i| u8::try_from(((produced + i) % 0x5F) + 0x20).unwrap_or(0x20).min(max_value))
                        .collect(),
                );
                produced += size;
            }
            packets
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoopbackResult {
    pub verdict: LoopbackVerdict,
    pub sent: Vec<u8>,
    pub received: Vec<u8>,
    pub duration_s: f64,
    pub mismatches: Vec<usize>,
    pub timed_out: bool,
    pub modem: ModemStatus,
    pub error: Option<UserFacingError>,
    pub hints: Vec<String>,
}

impl LoopbackResult {
    pub fn matched_bytes(&self) -> usize {
        self.sent.iter().zip(&self.received).filter(|(a, b)| a == b).count()
    }

    pub fn summary(&self) -> String {
        format!(
            "{}: {} von {} Bytes empfangen, {} Abweichung(en), Dauer {:.2} s{}",
            self.verdict.label(),
            self.received.len(),
            self.sent.len(),
            self.mismatches.len(),
            self.duration_s,
            if self.timed_out { ", Zeitüberschreitung" } else { "" }
        )
    }
}

fn traffic(events: &mut dyn EventSink, clock: &dyn Clock, direction: Direction, data: &[u8]) {
    if !data.is_empty() {
        events.emit(Event::Traffic(TrafficEvent { direction, timestamp: clock.now(), data: data.to_vec() }));
    }
}

fn hints() -> Vec<String> {
    FAILURE_HINTS.iter().map(|s| (*s).to_string()).collect()
}

/// Send `packets` and read them back. Never panics; transport errors become a failed result.
pub fn run_loopback_test(
    transport: &mut dyn Transport,
    packets: &[Vec<u8>],
    timeout_s: f64,
    clock: &dyn Clock,
    token: &CancelToken,
    events: &mut dyn EventSink,
) -> LoopbackResult {
    let sent: Vec<u8> = packets.concat();
    let mut received: Vec<u8> = Vec::new();
    let t0 = clock.monotonic();
    let mut modem = ModemStatus::default();
    let mut timed_out = false;
    let failed = |received: Vec<u8>, modem: ModemStatus, error: UserFacingError, hints: Vec<String>| LoopbackResult {
        verdict: LoopbackVerdict::Failed,
        sent: sent.clone(),
        received,
        duration_s: clock.monotonic() - t0,
        mismatches: Vec::new(),
        timed_out: false,
        modem,
        error: Some(error),
        hints,
    };
    let cancelled = || {
        let mut error = UserFacingError::new("Loopback-Test abgebrochen.");
        error.operation = "Loopback-Test".into();
        error
    };
    if let Err(err) = transport.open() {
        return failed(received, modem, describe(&err, "Loopback-Test", ""), hints());
    }
    transport.reset_input();
    modem = transport.modem_status();
    // Interleaved reads keep the receive buffer small; once nothing comes back they stop
    // (avoids one read timeout per packet when TX/RX are not bridged).
    let mut echo_seen = true;
    for packet in packets {
        let mut pos = 0usize;
        while pos < packet.len() {
            if token.is_cancelled() {
                transport.close();
                return failed(received, modem, cancelled(), Vec::new());
            }
            let chunk = packet.get(pos..).unwrap_or_default();
            match transport.write(chunk, token) {
                Ok(0) => {
                    transport.close();
                    return failed(received, modem, cancelled(), Vec::new());
                }
                Ok(written) => {
                    traffic(events, clock, Direction::Tx, chunk.get(..written).unwrap_or_default());
                    pos += written;
                }
                Err(err) => {
                    transport.close();
                    return failed(received, modem, describe(&err, "Loopback-Test", ""), hints());
                }
            }
        }
        if echo_seen && received.len() < sent.len() {
            match transport.read(sent.len() - received.len(), token) {
                Ok(data) => {
                    echo_seen = !data.is_empty();
                    traffic(events, clock, Direction::Rx, &data);
                    received.extend_from_slice(&data);
                }
                Err(err) => {
                    transport.close();
                    return failed(received, modem, describe(&err, "Loopback-Test", ""), hints());
                }
            }
        }
    }
    let deadline = clock.monotonic() + timeout_s;
    while received.len() < sent.len() {
        if token.is_cancelled() {
            transport.close();
            return failed(received, modem, cancelled(), Vec::new());
        }
        if clock.monotonic() >= deadline {
            timed_out = true;
            break;
        }
        match transport.read(sent.len() - received.len(), token) {
            Ok(data) => {
                traffic(events, clock, Direction::Rx, &data);
                received.extend_from_slice(&data);
            }
            Err(err) => {
                transport.close();
                return failed(received, modem, describe(&err, "Loopback-Test", ""), hints());
            }
        }
    }
    modem = transport.modem_status();
    transport.close();
    let mismatches: Vec<usize> =
        sent.iter().zip(&received).enumerate().filter(|(_, (a, b))| a != b).map(|(i, _)| i).collect();
    let verdict = if received == sent {
        LoopbackVerdict::Passed
    } else if received.is_empty() {
        LoopbackVerdict::Failed
    } else {
        LoopbackVerdict::Partial
    };
    LoopbackResult {
        verdict,
        duration_s: clock.monotonic() - t0,
        hints: if verdict == LoopbackVerdict::Passed { Vec::new() } else { hints() },
        sent,
        received,
        mismatches,
        timed_out,
        modem,
        error: None,
    }
}

// ------------------------------------------------------------ format detection

pub const MIN_SAMPLE: usize = 32;
const CONFIDENT_SHARE: f64 = 0.97;
const PRINTABLE_OK: f64 = 0.85;

/// 8N1 at the profile's baud rate; flow control kept so the CNC is allowed to send.
pub fn capture_settings(serial: &SerialSettings) -> SerialSettings {
    SerialSettings { data_bits: 8, parity: Parity::None, stop_bits: StopBits::One, ..serial.clone() }
}

fn even_parity_bit(value: u8) -> u8 {
    u8::try_from((value & 0x7F).count_ones() & 1).unwrap_or(0)
}

fn printable(value: u8) -> bool {
    let low = value & 0x7F;
    (0x20..0x7F).contains(&low) || matches!(low, 0x0A | 0x0D | 0x09 | 0x00 | 0x25)
}

#[derive(Debug, Clone, PartialEq)]
pub struct FormatGuess {
    pub sample_size: usize,
    pub printable_share: f64,
    pub even_share: f64,
    pub odd_share: f64,
    pub msb_one_share: f64,
    pub msb_zero_share: f64,
    pub data_bits: Option<u8>,
    pub parity: Option<Parity>,
    pub stop_bits: Option<u8>,
    /// "hoch" | "mittel" | "keine"
    pub confidence: &'static str,
    pub explanation: Vec<String>,
}

/// Python's `f"{x:.0%}"`: both round the exact decimal value of `x * 100` (ties to even).
fn percent(value: f64) -> String {
    format!("{:.0}%", value * 100.0)
}

impl FormatGuess {
    pub fn frame(&self) -> String {
        match (self.data_bits, self.parity, self.stop_bits) {
            (Some(bits), Some(parity), Some(stop)) => format!("{bits}{}{stop}", parity.letter()),
            _ => "unbestimmt".into(),
        }
    }

    pub fn report(&self) -> String {
        let mut lines = vec![
            format!("Ausgewertete Zeichen: {}", self.sample_size),
            format!("Lesbare Zeichen (Baudrate plausibel): {}", percent(self.printable_share)),
            format!("Bit 7 passt zu gerader Parität: {}", percent(self.even_share)),
            format!("Bit 7 passt zu ungerader Parität: {}", percent(self.odd_share)),
            format!("Bit 7 immer 1 (Stoppbit/Mark): {}", percent(self.msb_one_share)),
            String::new(),
            format!("Ergebnis: {} (Sicherheit: {})", self.frame(), self.confidence),
        ];
        lines.extend(self.explanation.iter().cloned());
        lines.join("\n")
    }
}

#[allow(clippy::cast_precision_loss)]
fn share(count: usize, total: usize) -> f64 {
    count as f64 / total as f64
}

/// Evaluate bytes received as 8N1. Pure function.
pub fn analyze_capture(data: &[u8]) -> FormatGuess {
    let sample: Vec<u8> = data.iter().copied().filter(|b| !matches!(b, 0x00 | 0x80 | 0xFF)).collect();
    let n = sample.len();
    let undetermined = |p: f64, e: f64, o: f64, one: f64, zero: f64, notes: Vec<String>| FormatGuess {
        sample_size: n,
        printable_share: p,
        even_share: e,
        odd_share: o,
        msb_one_share: one,
        msb_zero_share: zero,
        data_bits: None,
        parity: None,
        stop_bits: None,
        confidence: "keine",
        explanation: notes,
    };
    if n < MIN_SAMPLE {
        return undetermined(
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            vec![format!(
                "Zu wenige Daten (mindestens {MIN_SAMPLE} Zeichen nötig). Bitte ein längeres Programm von der CNC ausgeben."
            )],
        );
    }
    let printable_share = share(sample.iter().filter(|b| printable(**b)).count(), n);
    let even = share(sample.iter().filter(|b| (**b >> 7) == even_parity_bit(**b)).count(), n);
    let odd = share(sample.iter().filter(|b| (**b >> 7) != even_parity_bit(**b)).count(), n);
    let msb_one = share(sample.iter().filter(|b| **b & 0x80 != 0).count(), n);
    let msb_zero = 1.0 - msb_one;
    let mut notes = Vec::new();
    if printable_share < PRINTABLE_OK {
        notes.push(
            "Viele unlesbare Zeichen: Die Baudrate im Profil passt vermutlich nicht. Andere Baudrate einstellen und erneut mitlesen."
                .into(),
        );
        return undetermined(printable_share, even, odd, msb_one, msb_zero, notes);
    }
    let mut data_bits = 7u8;
    let parity = if msb_one >= CONFIDENT_SHARE {
        notes.push(
            "Bit 7 ist praktisch immer 1: kein Paritätsbit (7N1 oder 7N2). Mark-Parität (7M1) ist davon nicht unterscheidbar."
                .into(),
        );
        Parity::None
    } else if msb_zero >= CONFIDENT_SHARE {
        data_bits = 8;
        notes.push("Bit 7 ist praktisch immer 0: 8 Datenbits ohne Parität (8N1) oder Space-Parität (7S1).".into());
        Parity::None
    } else if even >= CONFIDENT_SHARE {
        notes.push("Bit 7 entspricht der geraden Parität: 7E1.".into());
        Parity::Even
    } else if odd >= CONFIDENT_SHARE {
        notes.push("Bit 7 entspricht der ungeraden Parität: 7O1.".into());
        Parity::Odd
    } else {
        notes.push(
            "Kein eindeutiges Muster. Mögliche Ursachen: Übertragungsstörungen, Rahmenfehler bei 7N1 oder 8 Datenbits mit Sonderzeichen."
                .into(),
        );
        return undetermined(printable_share, even, odd, msb_one, msb_zero, notes);
    };
    let confidence = if n >= 200 && printable_share >= 0.95 { "hoch" } else { "mittel" };
    notes.push(
        "Das Ergebnis ist ein Hinweis. Bestätigen Sie es durch einen normalen Empfang mit dem passenden Profil und tragen Sie es in die Verifikations-Checkliste ein."
            .into(),
    );
    FormatGuess {
        sample_size: n,
        printable_share,
        even_share: even,
        odd_share: odd,
        msb_one_share: msb_one,
        msb_zero_share: msb_zero,
        data_bits: Some(data_bits),
        parity: Some(parity),
        stop_bits: Some(1),
        confidence,
        explanation: notes,
    }
}

/// Read only - never writes. Ends after `duration_s`, `max_bytes` or cancel.
pub fn capture_passively(
    transport: &mut dyn Transport,
    duration_s: f64,
    max_bytes: usize,
    clock: &dyn Clock,
    token: &CancelToken,
    events: &mut dyn EventSink,
) -> Result<Vec<u8>, UserFacingError> {
    transport.open().map_err(|e| describe(&e, "Datenformat mitlesen", ""))?;
    let mut buffer = Vec::new();
    let deadline = clock.monotonic() + duration_s;
    while clock.monotonic() < deadline && buffer.len() < max_bytes && !token.is_cancelled() {
        match transport.read(256, token) {
            Ok(chunk) => {
                traffic(events, clock, Direction::Rx, &chunk);
                buffer.extend_from_slice(&chunk);
            }
            Err(err) => {
                transport.close();
                return Err(describe(&err, "Datenformat mitlesen", ""));
            }
        }
    }
    transport.close();
    buffer.truncate(max_bytes);
    Ok(buffer)
}

/// How a 7-bit stream with `parity` looks when read as 8N1 (simulator / tests).
pub fn encode_frames_as_8n1(text: &[u8], parity: Parity) -> Vec<u8> {
    text.iter()
        .map(|value| {
            let low = value & 0x7F;
            let bit = match parity {
                Parity::Even => even_parity_bit(low),
                Parity::Odd => 1 - even_parity_bit(low),
                Parity::Space => 0,
                Parity::None | Parity::Mark => 1,
            };
            low | (bit << 7)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FakeClock;
    use crate::profile::builtin_profiles;
    use crate::scenarios::SAMPLE_PROGRAM;
    use crate::transfer::NoEvents;
    use crate::transport::loopback::LoopbackTransport;
    use std::sync::Arc;

    #[test]
    fn patterns_like_python() {
        let ascii = make_pattern(LoopbackPattern::AsciiText, 5, 7);
        assert_eq!(ascii, vec![b"DECKE".to_vec()]);
        let range = make_pattern(LoopbackPattern::ByteRange, 130, 7);
        assert_eq!(range[0][127], 127);
        assert_eq!(range[0][128], 0);
        let packets = make_pattern(LoopbackPattern::VariablePackets, 12, 7);
        assert_eq!(packets.iter().map(Vec::len).collect::<Vec<_>>(), vec![1, 2, 7, 2]);
        assert_eq!(packets[0], vec![0x20]);
    }

    #[test]
    fn loopback_bridged_and_open() {
        let serial = builtin_profiles().0.into_iter().next().unwrap().serial;
        let clock = FakeClock::default();
        let packets = make_pattern(LoopbackPattern::VariablePackets, 300, 7);
        let mut bridged = LoopbackTransport::new(serial.clone(), Arc::new(clock.clone()));
        let result = run_loopback_test(&mut bridged, &packets, 1.0, &clock, &CancelToken::new(), &mut NoEvents);
        assert_eq!(result.verdict, LoopbackVerdict::Passed, "{}", result.summary());
        assert!(result.hints.is_empty());
        let mut open = LoopbackTransport::new(serial, Arc::new(clock.clone()));
        open.bridged = false;
        let result = run_loopback_test(&mut open, &packets, 1.0, &clock, &CancelToken::new(), &mut NoEvents);
        assert_eq!(result.verdict, LoopbackVerdict::Failed);
        assert!(result.timed_out && result.hints.len() == 7);
    }

    #[test]
    fn detects_parity() {
        let stream = SAMPLE_PROGRAM.repeat(4);
        assert_eq!(analyze_capture(&encode_frames_as_8n1(&stream, Parity::Even)).frame(), "7E1");
        assert_eq!(analyze_capture(&encode_frames_as_8n1(&stream, Parity::Odd)).frame(), "7O1");
        assert_eq!(analyze_capture(&encode_frames_as_8n1(&stream, Parity::None)).frame(), "7N1");
        assert_eq!(analyze_capture(&stream).frame(), "8N1");
        let short = analyze_capture(b"N10");
        assert_eq!(short.confidence, "keine");
        assert!(analyze_capture(&encode_frames_as_8n1(&stream, Parity::Even)).report().contains("Sicherheit: hoch"));
        assert_eq!(percent(0.125), "12%");
        assert_eq!(percent(0.135), "14%");
        assert_eq!(percent(0.575), "57%");
        assert_eq!(percent(0.015), "2%");
    }
}
