//! LSV2 transfer, drip-feed and receive against the simulated LSV2 CNC (ported from the Python tests).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::sync::Arc;

use deckellink_core::clock::{CancelToken, Clock, FakeClock};
use deckellink_core::model::{BccMode, Profile};
use deckellink_core::nc::transformer::transform;
use deckellink_core::nc::TransformResult;
use deckellink_core::profile::builtin_profiles;
use deckellink_core::transfer::lsv2_transfer::{build_telegrams, Lsv2ProgramReceiver, Lsv2ProgramSender};
use deckellink_core::transfer::receiver::process_received;
use deckellink_core::transfer::{
    Direction, EndReason, Event, EventSink, NoEvents, ReceiveResult, ReceiveState, SendResult, SendState,
};
use deckellink_core::transport::lsv2_sim::{CncRecord, Lsv2SimConfig, Lsv2SimulatedCnc};
use deckellink_core::transport::Transport;

const SAMPLE_PROGRAM: &[u8] =
    b"(DECKELLINK SIMULATOR TESTPROGRAMM)\r\nN10 G90 G17\r\nN20 G00 X0 Y0 Z50\r\nN30 T1 M06\r\n\
N40 S1200 M03\r\nN50 G01 Z-1 F100\r\nN60 X40 F200\r\nN70 Y30\r\nN80 X0\r\nN90 Y0\r\nN100 G00 Z50 M05\r\nN110 M30\r\n";

fn lsv2() -> Profile {
    builtin_profiles().0.into_iter().find(|p| p.id == "dialog11_lsv2_7e1").unwrap()
}

fn payload(profile: &Profile, data: &[u8]) -> TransformResult {
    transform(data, &profile.transfer, false).unwrap()
}

/// Collects events and optionally cancels after `cancel_after` transmitted bytes.
struct Recorder {
    events: Vec<Event>,
    cancel_after: Option<usize>,
    sent: usize,
    token: CancelToken,
}

impl EventSink for Recorder {
    fn emit(&mut self, event: Event) {
        if let Event::Traffic(t) = &event {
            if t.direction == Direction::Tx {
                self.sent += t.data.len();
                if self.cancel_after.is_some_and(|limit| self.sent >= limit) {
                    self.token.cancel();
                }
            }
        }
        self.events.push(event);
    }
}

struct Run {
    result: SendResult,
    record: CncRecord,
    events: Vec<Event>,
    cnc_open: bool,
}

fn send(profile: &Profile, config: Lsv2SimConfig, data: &[u8], drip: bool, cancel_after: Option<usize>) -> Run {
    send_with_cnc_protocol(profile, profile, config, data, drip, cancel_after)
}

fn send_with_cnc_protocol(
    profile: &Profile,
    cnc_profile: &Profile,
    config: Lsv2SimConfig,
    data: &[u8],
    drip: bool,
    cancel_after: Option<usize>,
) -> Run {
    let clock = FakeClock::default();
    let mut cnc = Lsv2SimulatedCnc::new(
        cnc_profile.serial.clone(),
        cnc_profile.protocol.clone(),
        config,
        Arc::new(clock.clone()),
    );
    let token = CancelToken::new();
    let mut recorder = Recorder { events: Vec::new(), cancel_after, sent: 0, token: token.clone() };
    let prepared = payload(profile, data);
    let mut sender = Lsv2ProgramSender::new(&mut cnc, profile, &clock, &token, &mut recorder, drip);
    sender.program_name = "TEIL1".into();
    let result = sender.send(&prepared);
    Run { result, record: cnc.record(), events: recorder.events, cnc_open: cnc.is_open() }
}

fn receive(profile: &Profile, config: Lsv2SimConfig, cancelled: bool) -> (ReceiveResult, CncRecord) {
    let clock = FakeClock::default();
    let mut cnc =
        Lsv2SimulatedCnc::new(profile.serial.clone(), profile.protocol.clone(), config, Arc::new(clock.clone()));
    let token = CancelToken::new();
    if cancelled {
        token.cancel();
    }
    let mut events = NoEvents;
    let result = Lsv2ProgramReceiver::new(&mut cnc, profile, &clock, &token, &mut events).receive();
    (result, cnc.record())
}

fn states(events: &[Event]) -> Vec<SendState> {
    events.iter().filter_map(|e| if let Event::SendState(s) = e { Some(*s) } else { None }).collect()
}

#[test]
fn build_telegrams_per_line_grouped_and_limited() {
    let p = lsv2();
    let prepared = payload(&p, SAMPLE_PROGRAM);
    let telegrams = build_telegrams(&prepared, &p).unwrap();
    assert_eq!(telegrams.len(), prepared.line_count);
    assert_eq!(telegrams.iter().flat_map(|(t, _)| t.clone()).collect::<Vec<_>>(), prepared.data);
    assert_eq!(telegrams.last().unwrap().1, prepared.data.len());
    let mut grouped = p.clone();
    grouped.protocol.lines_per_telegram = 5;
    grouped.protocol.include_line_ending = false;
    let g = build_telegrams(&prepared, &grouped).unwrap();
    assert_eq!(g.len(), 3);
    assert!(!g[0].0.contains(&b'\r'));
    let mut tiny = p;
    tiny.protocol.max_telegram_length = 8;
    assert!(build_telegrams(&prepared, &tiny).unwrap_err().contains("Zeile"));
}

#[test]
fn memory_transfer() {
    let p = lsv2();
    let run = send(&p, Lsv2SimConfig::default(), SAMPLE_PROGRAM, false, None);
    assert_eq!(run.result.state, SendState::Completed, "{:?}", run.result.error);
    let prepared = payload(&p, SAMPLE_PROGRAM);
    assert_eq!(run.record.received_program(), prepared.data);
    assert_eq!(run.record.telegrams_received, prepared.line_count);
    assert!(run.record.eot_received && !run.cnc_open);
    let s = states(&run.events);
    assert!(s.contains(&SendState::Sending) && !s.contains(&SendState::DripFeeding));
    let sent: Vec<u8> = run
        .events
        .iter()
        .filter_map(|e| {
            if let Event::Traffic(t) = e {
                (t.direction == Direction::Tx).then(|| t.data.clone())
            } else {
                None
            }
        })
        .flatten()
        .collect();
    assert_eq!(sent.first(), Some(&0x05));
    assert_eq!(sent.last(), Some(&0x04));
}

#[test]
fn drip_feed_is_paced_by_cnc() {
    let p = lsv2();
    let config = Lsv2SimConfig { buffer_telegrams: 3, execution_s_per_telegram: 2.0, ..Lsv2SimConfig::default() };
    let run = send(&p, config, SAMPLE_PROGRAM, true, None);
    let prepared = payload(&p, SAMPLE_PROGRAM);
    assert_eq!(run.result.state, SendState::Completed);
    assert!(states(&run.events).contains(&SendState::DripFeeding));
    assert_eq!(run.record.received_program(), prepared.data);
    #[allow(clippy::cast_precision_loss)]
    let minimum = (prepared.line_count - 3) as f64 * 2.0;
    assert!(run.result.duration_s >= minimum, "{} < {minimum}", run.result.duration_s);
    let last_line =
        run.events.iter().rev().find_map(|e| if let Event::Progress(p) = e { Some(p.current_line) } else { None });
    assert_eq!(last_line, Some(prepared.line_count));
}

#[test]
fn drip_feed_waits_longer_than_normal_ack_timeout() {
    let p = lsv2();
    let config = Lsv2SimConfig { buffer_telegrams: 1, execution_s_per_telegram: 30.0, ..Lsv2SimConfig::default() };
    let data = b"N10\r\nN20\r\nN30 M30\r\n";
    assert_eq!(send(&p, config.clone(), data, true, None).result.state, SendState::Completed);
    let normal = send(&p, config, data, false, None).result;
    assert_eq!(normal.state, SendState::Failed);
    assert!(normal.error.unwrap().title.contains("quittiert"));
}

#[test]
fn faults_nak_lost_ack_wack_do_not_duplicate() {
    let p = lsv2();
    let prepared = payload(&p, SAMPLE_PROGRAM);
    for config in [
        Lsv2SimConfig { nak_on_telegrams: vec![2, 5], ..Lsv2SimConfig::default() },
        Lsv2SimConfig { drop_ack_on_telegrams: vec![4], ..Lsv2SimConfig::default() },
        Lsv2SimConfig { wack_on_telegrams: vec![2, 3], ..Lsv2SimConfig::default() },
    ] {
        for drip in [false, true] {
            let run = send(&p, config.clone(), SAMPLE_PROGRAM, drip, None);
            assert_eq!(run.result.state, SendState::Completed, "{config:?}");
            assert_eq!(run.record.telegrams_received, prepared.line_count);
            assert_eq!(run.record.received_program(), prepared.data);
        }
    }
    let nak = send(
        &p,
        Lsv2SimConfig { nak_on_telegrams: vec![2, 5], ..Lsv2SimConfig::default() },
        SAMPLE_PROGRAM,
        false,
        None,
    );
    assert_eq!(nak.record.naks_sent, 2);
}

#[test]
fn retry_limit_on_bcc_mismatch() {
    let mut p = lsv2();
    p.protocol.max_retries = 1;
    let mut cnc_profile = p.clone();
    cnc_profile.protocol.bcc_mode = BccMode::Transmitted;
    let run = send_with_cnc_protocol(&p, &cnc_profile, Lsv2SimConfig::default(), b"N10 X1\r\nM30\r\n", false, None);
    assert_eq!(run.result.state, SendState::Failed);
    assert!(run.result.error.unwrap().title.contains("NAK"));
}

#[test]
fn cnc_abort_not_ready_disconnect_and_open_errors() {
    let p = lsv2();
    let abort = send(
        &p,
        Lsv2SimConfig { eot_after_telegrams: Some(5), ..Lsv2SimConfig::default() },
        SAMPLE_PROGRAM,
        true,
        None,
    );
    assert!(abort.result.error.unwrap().title.contains("EOT"));
    assert_eq!(abort.record.telegrams_received, 5);
    let mut short = p.clone();
    short.protocol.link_timeout_ms = 5000;
    let not_ready =
        send(&short, Lsv2SimConfig { never_ready: true, ..Lsv2SimConfig::default() }, SAMPLE_PROGRAM, false, None);
    assert!(not_ready.result.error.unwrap().title.contains("DLE 0"));
    assert!((5.0..=8.0).contains(&not_ready.result.duration_s), "{}", not_ready.result.duration_s);
    let lost = Lsv2SimConfig {
        disconnect_after_telegrams: Some(4),
        buffer_telegrams: 2,
        execution_s_per_telegram: 0.2,
        ..Lsv2SimConfig::default()
    };
    for drip in [false, true] {
        assert_eq!(send(&p, lost.clone(), SAMPLE_PROGRAM, drip, None).result.state, SendState::Failed);
    }
    let busy = send(
        &p,
        Lsv2SimConfig { open_error: Some("busy".into()), ..Lsv2SimConfig::default() },
        SAMPLE_PROGRAM,
        false,
        None,
    );
    assert!(busy.result.error.unwrap().title.contains("belegt"));
}

#[test]
fn user_cancel_sends_eot_unless_disabled() {
    let p = lsv2();
    let config = Lsv2SimConfig { buffer_telegrams: 3, execution_s_per_telegram: 0.5, ..Lsv2SimConfig::default() };
    let prepared = payload(&p, SAMPLE_PROGRAM);
    let run = send(&p, config.clone(), SAMPLE_PROGRAM, true, Some(60));
    assert_eq!(run.result.state, SendState::Cancelled);
    assert!(run.record.eot_received);
    assert!(run.record.telegrams_received < prepared.line_count);
    let mut no_eot = p;
    no_eot.protocol.send_eot_on_cancel = false;
    let run = send(&no_eot, config, SAMPLE_PROGRAM, true, Some(60));
    assert!(!run.record.eot_received);
}

#[test]
fn start_end_telegrams_and_handshake() {
    let mut p = lsv2();
    p.protocol.start_telegrams = vec!["%{name}".into()];
    p.protocol.end_telegrams = vec!["END".into()];
    let run = send(&p, Lsv2SimConfig::default(), b"N10\r\nM30\r\n", false, None);
    assert_eq!(run.record.received_texts.first().map(Vec::as_slice), Some(&b"%TEIL1"[..]));
    assert_eq!(run.record.received_texts.last().map(Vec::as_slice), Some(&b"END"[..]));
    let mut hs = lsv2();
    hs.serial.require_cts_before_send = true;
    hs.serial.handshake_timeout_ms = 1000;
    let run = send(&hs, Lsv2SimConfig { cts: false, ..Lsv2SimConfig::default() }, b"M30\r\n", false, None);
    assert_eq!(run.result.state, SendState::Failed);
    assert!(run.result.error.unwrap().title.contains("CTS"));
}

#[test]
fn receive_program_and_faults() {
    let p = lsv2();
    let (result, record) =
        receive(&p, Lsv2SimConfig { program: SAMPLE_PROGRAM.to_vec(), ..Lsv2SimConfig::default() }, false);
    assert_eq!((result.state, result.end_reason), (ReceiveState::Completed, EndReason::Eot));
    assert_eq!(result.raw, SAMPLE_PROGRAM);
    assert!(process_received(&result, &p).complete);
    assert!(record.sent_eot);
    let mut no_endings = p.clone();
    no_endings.protocol.include_line_ending = false;
    let corrupted = Lsv2SimConfig {
        nak_on_telegrams: vec![2, 5],
        corrupt_bcc_on_sent_telegrams: vec![3],
        program: SAMPLE_PROGRAM.to_vec(),
        ..Lsv2SimConfig::default()
    };
    let (result, _) = receive(&no_endings, corrupted, false);
    assert_eq!(result.state, ReceiveState::Completed);
    assert_eq!(result.raw, SAMPLE_PROGRAM);
    let mut quiet = p.clone();
    quiet.receive.first_byte_timeout_ms = 3000;
    let (result, _) = receive(
        &quiet,
        Lsv2SimConfig { program: SAMPLE_PROGRAM.to_vec(), never_ready: true, ..Lsv2SimConfig::default() },
        false,
    );
    assert_eq!((result.state, result.end_reason), (ReceiveState::TimedOut, EndReason::NoData));
    let mut small = p.clone();
    small.receive.max_receive_bytes = 50;
    let (result, _) =
        receive(&small, Lsv2SimConfig { program: SAMPLE_PROGRAM.to_vec(), ..Lsv2SimConfig::default() }, false);
    assert_eq!((result.state, result.end_reason), (ReceiveState::Failed, EndReason::MaxSize));
    assert!(result.raw.len() <= 50);
    let (result, _) = receive(&p, Lsv2SimConfig { program: SAMPLE_PROGRAM.to_vec(), ..Lsv2SimConfig::default() }, true);
    assert_eq!(result.state, ReceiveState::Cancelled);
    let mut eot_file = SAMPLE_PROGRAM.to_vec();
    eot_file.extend_from_slice(b"\x04\r\n");
    let (result, _) = receive(&p, Lsv2SimConfig { program: eot_file, ..Lsv2SimConfig::default() }, false);
    assert_eq!(result.raw, SAMPLE_PROGRAM);
}

#[test]
fn eot_trailer_of_file_is_replaced_by_link_eot() {
    let p = lsv2();
    let mut data = SAMPLE_PROGRAM.to_vec();
    data.extend_from_slice(b"\x04\r\n");
    let run = send(&p, Lsv2SimConfig::default(), &data, false, None);
    assert_eq!(run.result.state, SendState::Completed);
    assert_eq!(run.result.bytes_sent, run.result.bytes_total);
    assert_eq!(run.record.received_program(), SAMPLE_PROGRAM);
    let clock = FakeClock::default();
    assert!(clock.monotonic() >= 0.0);
}
