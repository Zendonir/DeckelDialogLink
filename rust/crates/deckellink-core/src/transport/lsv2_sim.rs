//! Simulated CNC speaking LSV2 (DIN 66019 basic mode).
//!
//! Models the CNC as receiver (memory transfer and drip-feed with a limited block
//! buffer consumed at a fixed execution rate) and as sender (program output).
//! Faults are reproducible through [`Lsv2SimConfig`].

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, PoisonError};

use crate::clock::{CancelToken, Clock};
use crate::model::{ProtocolSettings, SerialSettings};
use crate::nc::checks::{split_eot_trailer, split_lines};
use crate::transfer::lsv2::{encode_telegram, EventKind, FrameDecoder, ACK0, ACK1, ENQ, EOT, NAK, WACK};
use crate::transport::simulated::SIMULATED_PORT_NAME;
use crate::transport::{ErrorKind, ModemStatus, Transport, TransportError};

#[derive(Debug, Clone, PartialEq)]
pub struct Lsv2SimConfig {
    pub speed_factor: f64,
    pub open_error: Option<String>,
    pub ready_after_s: f64,
    pub never_ready: bool,
    pub response_delay_s: f64,
    /// 0 = memory transfer: acknowledge immediately
    pub buffer_telegrams: usize,
    /// drip-feed: time the CNC needs per block
    pub execution_s_per_telegram: f64,
    pub nak_on_telegrams: Vec<usize>,
    pub drop_ack_on_telegrams: Vec<usize>,
    pub wack_on_telegrams: Vec<usize>,
    pub eot_after_telegrams: Option<usize>,
    pub disconnect_after_telegrams: Option<usize>,
    /// Data the CNC sends when the PC receives.
    pub program: Vec<u8>,
    pub send_lines_per_telegram: usize,
    pub corrupt_bcc_on_sent_telegrams: Vec<usize>,
    pub cts: bool,
    pub dsr: bool,
}

impl Default for Lsv2SimConfig {
    fn default() -> Self {
        Self {
            speed_factor: 1.0,
            open_error: None,
            ready_after_s: 0.5,
            never_ready: false,
            response_delay_s: 0.02,
            buffer_telegrams: 0,
            execution_s_per_telegram: 0.0,
            nak_on_telegrams: Vec::new(),
            drop_ack_on_telegrams: Vec::new(),
            wack_on_telegrams: Vec::new(),
            eot_after_telegrams: None,
            disconnect_after_telegrams: None,
            program: Vec::new(),
            send_lines_per_telegram: 1,
            corrupt_bcc_on_sent_telegrams: Vec::new(),
            cts: true,
            dsr: true,
        }
    }
}

/// What the simulated CNC received (shared, readable after the transport moved into a worker).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CncRecord {
    pub telegrams_received: usize,
    pub received_texts: Vec<Vec<u8>>,
    pub eot_received: bool,
    pub sent_eot: bool,
    pub naks_sent: usize,
}

impl CncRecord {
    pub fn received_program(&self) -> Vec<u8> {
        self.received_texts.concat()
    }
}

/// Transport implementation acting as an LSV2 speaking CNC.
#[derive(Debug)]
pub struct Lsv2SimulatedCnc {
    settings: SerialSettings,
    protocol: ProtocolSettings,
    pub config: Lsv2SimConfig,
    clock: Arc<dyn Clock>,
    rate: f64,
    open: bool,
    outgoing: Vec<(f64, u64, Vec<u8>)>,
    seq: u64,
    buffer: Vec<u8>,
    decoder: FrameDecoder,
    opened_at: f64,
    linked: bool,
    last_ack: [u8; 2],
    next_ack: [u8; 2],
    nak_done: BTreeSet<usize>,
    drop_done: BTreeSet<usize>,
    wack_done: BTreeSet<usize>,
    exec_start: Option<f64>,
    last_ack_time: f64,
    disconnected: bool,
    record: Arc<Mutex<CncRecord>>,
    send_telegrams: Vec<Vec<u8>>,
    send_index: usize,
    send_linked: bool,
    next_enq_at: f64,
    send_expected: [u8; 2],
    corrupted: BTreeSet<usize>,
}

impl Lsv2SimulatedCnc {
    pub fn new(
        settings: SerialSettings,
        protocol: ProtocolSettings,
        config: Lsv2SimConfig,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let rate = (settings.bytes_per_second() * config.speed_factor.max(0.001)).max(1.0);
        let decoder = FrameDecoder::new(&protocol, 65_536);
        let send_telegrams = split_program(&config, &protocol);
        Self {
            settings,
            protocol,
            config,
            clock,
            rate,
            open: false,
            outgoing: Vec::new(),
            seq: 0,
            buffer: Vec::new(),
            decoder,
            opened_at: 0.0,
            linked: false,
            last_ack: ACK0,
            next_ack: ACK1,
            nak_done: BTreeSet::new(),
            drop_done: BTreeSet::new(),
            wack_done: BTreeSet::new(),
            exec_start: None,
            last_ack_time: 0.0,
            disconnected: false,
            record: Arc::new(Mutex::new(CncRecord::default())),
            send_telegrams,
            send_index: 0,
            send_linked: false,
            next_enq_at: 0.0,
            send_expected: ACK1,
            corrupted: BTreeSet::new(),
        }
    }

    pub fn record(&self) -> CncRecord {
        self.record.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub fn record_handle(&self) -> Arc<Mutex<CncRecord>> {
        Arc::clone(&self.record)
    }

    pub fn is_sender(&self) -> bool {
        !self.send_telegrams.is_empty()
    }

    fn with_record<R>(&self, f: impl FnOnce(&mut CncRecord) -> R) -> R {
        let mut guard = self.record.lock().unwrap_or_else(PoisonError::into_inner);
        f(&mut guard)
    }

    fn require_open(&self) -> Result<(), TransportError> {
        if self.open && !self.disconnected {
            Ok(())
        } else {
            Err(TransportError::new(
                ErrorKind::ConnectionLost,
                "Simulierte LSV2-Verbindung ist getrennt.",
                SIMULATED_PORT_NAME,
            ))
        }
    }

    fn queue_at(&mut self, when: f64, data: Vec<u8>) {
        self.seq += 1;
        let key = (when, self.seq);
        let index = self.outgoing.partition_point(|(t, s, _)| t.total_cmp(&key.0).then(s.cmp(&key.1)).is_lt());
        self.outgoing.insert(index, (when, self.seq, data));
    }

    fn queue(&mut self, data: Vec<u8>) {
        let when = self.clock.monotonic() + self.config.response_delay_s;
        self.queue_at(when, data);
    }

    fn tick(&mut self) {
        if !self.is_sender() || self.send_linked || self.with_record(|r| r.sent_eot) {
            return;
        }
        let now = self.clock.monotonic();
        if now >= self.next_enq_at && !self.config.never_ready {
            self.queue_at(now, vec![ENQ]);
            self.next_enq_at = now + crate::transfer::lsv2::ms(self.protocol.enq_interval_ms);
        }
    }

    fn handle_as_receiver(&mut self, kind: EventKind, text: Vec<u8>, bcc_ok: bool) {
        let now = self.clock.monotonic();
        match kind {
            EventKind::Enq => {
                if !self.linked {
                    if self.config.never_ready || now - self.opened_at < self.config.ready_after_s {
                        return;
                    }
                    self.linked = true;
                    self.last_ack = ACK0;
                    self.next_ack = ACK1;
                    self.queue(ACK0.to_vec());
                } else {
                    let when = (now + self.config.response_delay_s).max(self.last_ack_time);
                    let ack = self.last_ack.to_vec();
                    self.queue_at(when, ack);
                }
                return;
            }
            EventKind::Eot => {
                self.with_record(|r| r.eot_received = true);
                self.linked = false;
                return;
            }
            EventKind::Telegram if self.linked => {}
            _ => return,
        }
        let number = self.with_record(|r| r.telegrams_received) + 1;
        if !bcc_ok || (self.config.nak_on_telegrams.contains(&number) && !self.nak_done.contains(&number)) {
            self.nak_done.insert(number);
            self.with_record(|r| r.naks_sent += 1);
            self.queue(vec![NAK]);
            return;
        }
        self.with_record(|r| {
            r.telegrams_received = number;
            r.received_texts.push(text);
        });
        let ack = self.next_ack;
        self.last_ack = ack;
        self.next_ack = if ack == ACK1 { ACK0 } else { ACK1 };
        let mut ack_time = now + self.config.response_delay_s;
        if self.config.buffer_telegrams > 0 && self.config.execution_s_per_telegram > 0.0 {
            let start = *self.exec_start.get_or_insert(now);
            #[allow(clippy::cast_precision_loss)]
            let free_at =
                start + (number as f64 - self.config.buffer_telegrams as f64) * self.config.execution_s_per_telegram;
            ack_time = ack_time.max(free_at);
        }
        self.last_ack_time = ack_time;
        if self.config.disconnect_after_telegrams.is_some_and(|limit| number >= limit) {
            self.disconnected = true;
            return;
        }
        if self.config.drop_ack_on_telegrams.contains(&number) && !self.drop_done.contains(&number) {
            self.drop_done.insert(number);
            return;
        }
        if self.config.wack_on_telegrams.contains(&number) && !self.wack_done.contains(&number) {
            self.wack_done.insert(number);
            self.queue_at(ack_time, WACK.to_vec());
            return;
        }
        self.queue_at(ack_time, ack.to_vec());
        if self.config.eot_after_telegrams.is_some_and(|limit| number >= limit) {
            self.queue_at(ack_time + 0.01, vec![EOT]);
            self.with_record(|r| r.sent_eot = true);
            self.linked = false;
        }
    }

    fn send_current(&mut self) {
        let number = self.send_index + 1;
        let Some(text) = self.send_telegrams.get(self.send_index) else { return };
        let Ok(mut frame) = encode_telegram(text, &self.protocol) else { return };
        if self.config.corrupt_bcc_on_sent_telegrams.contains(&number) && !self.corrupted.contains(&number) {
            self.corrupted.insert(number);
            if let Some(last) = frame.last_mut() {
                *last ^= 0x01;
            }
        }
        self.queue(frame);
    }

    fn handle_as_sender(&mut self, kind: EventKind) {
        if self.with_record(|r| r.sent_eot) {
            return;
        }
        if !self.send_linked {
            if kind == EventKind::Ack0 {
                self.send_linked = true;
                self.send_expected = ACK1;
                self.send_current();
            }
            return;
        }
        let expected_kind = if self.send_expected == ACK1 { EventKind::Ack1 } else { EventKind::Ack0 };
        if kind == expected_kind {
            self.send_index += 1;
            self.send_expected = if self.send_expected == ACK1 { ACK0 } else { ACK1 };
            if self.send_index >= self.send_telegrams.len() {
                self.queue(vec![EOT]);
                self.with_record(|r| r.sent_eot = true);
            } else {
                self.send_current();
            }
        } else if kind == EventKind::Nak {
            self.send_current();
        }
    }
}

fn split_program(config: &Lsv2SimConfig, protocol: &ProtocolSettings) -> Vec<Vec<u8>> {
    let (data, _trailer) = split_eot_trailer(&config.program);
    if data.is_empty() {
        return Vec::new();
    }
    let lines: Vec<Vec<u8>> = split_lines(data)
        .into_iter()
        .map(|l| {
            let mut v = l.content;
            v.extend_from_slice(&l.ending);
            v
        })
        .collect();
    let step = config.send_lines_per_telegram.max(1);
    lines
        .chunks(step)
        .map(|chunk| {
            let text = chunk.concat();
            if protocol.include_line_ending {
                text
            } else {
                let end = text.iter().rposition(|b| !matches!(*b, b'\r' | b'\n')).map_or(0, |p| p + 1);
                text.get(..end).unwrap_or_default().to_vec()
            }
        })
        .collect()
}

impl Transport for Lsv2SimulatedCnc {
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
        let error = match self.config.open_error.as_deref() {
            Some("busy") => Some((ErrorKind::Busy, "Simulierter Fehler: Port belegt")),
            Some("not_found") => Some((ErrorKind::NotFound, "Simulierter Fehler: Port existiert nicht")),
            Some(_) => Some((ErrorKind::Open, "Simulierter Fehler beim Öffnen")),
            None => None,
        };
        if let Some((kind, message)) = error {
            return Err(TransportError::new(kind, message, SIMULATED_PORT_NAME));
        }
        self.open = true;
        self.opened_at = self.clock.monotonic();
        self.next_enq_at = self.opened_at + self.config.ready_after_s;
        Ok(())
    }

    fn close(&mut self) {
        self.open = false;
    }

    fn write(&mut self, data: &[u8], cancel: &CancelToken) -> Result<usize, TransportError> {
        self.require_open()?;
        if data.is_empty() {
            return Ok(0);
        }
        #[allow(clippy::cast_precision_loss)]
        if self.clock.sleep(data.len() as f64 / self.rate, Some(cancel)) {
            return Ok(0);
        }
        let mask = u8::try_from((1u16 << self.settings.data_bits.min(8)).saturating_sub(1)).unwrap_or(0xFF);
        let masked: Vec<u8> = data.iter().map(|b| b & mask).collect();
        for event in self.decoder.feed(&masked) {
            if self.is_sender() {
                self.handle_as_sender(event.kind);
            } else {
                self.handle_as_receiver(event.kind, event.text, event.bcc_ok);
            }
        }
        Ok(data.len())
    }

    fn read(&mut self, max: usize, cancel: &CancelToken) -> Result<Vec<u8>, TransportError> {
        self.require_open()?;
        let deadline = self.clock.monotonic() + crate::transfer::lsv2::ms(self.settings.read_timeout_ms);
        loop {
            if cancel.is_cancelled() {
                return Ok(Vec::new());
            }
            self.tick();
            let now = self.clock.monotonic();
            let due = self.outgoing.partition_point(|(t, _, _)| *t <= now);
            for (_, _, data) in self.outgoing.drain(..due) {
                self.buffer.extend_from_slice(&data);
            }
            if !self.buffer.is_empty() {
                let count = max.max(1).min(self.buffer.len());
                return Ok(self.buffer.drain(..count).collect());
            }
            if now >= deadline {
                return Ok(Vec::new());
            }
            let mut wake = deadline;
            if let Some((t, _, _)) = self.outgoing.first() {
                wake = wake.min(*t);
            }
            if self.is_sender() && !self.send_linked && !self.config.never_ready {
                wake = wake.min(self.next_enq_at.max(now));
            }
            if self.clock.sleep((wake - now).max(1e-4), Some(cancel)) {
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
            cts: Some(self.config.cts),
            dsr: Some(self.config.dsr),
            dcd: Some(false),
            ri: Some(false),
            rts: Some(true),
            dtr: Some(false),
        }
    }

    fn set_control_lines(&mut self, _rts: Option<bool>, _dtr: Option<bool>) -> Result<(), TransportError> {
        self.require_open()
    }

    fn reset_input(&mut self) {
        self.buffer.clear();
    }
}
