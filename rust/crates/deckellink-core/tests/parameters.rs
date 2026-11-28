//! Machine parameters: read into the archive, gate, write and verify against the simulated CNC
//! (plain protocol and LSV2). Mirrors the Python tests of the parameter workflow.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::sync::Arc;

use deckellink_core::clock::{CancelToken, Clock, FakeClock};
use deckellink_core::model::Profile;
use deckellink_core::parameters::archive::ParameterArchive;
use deckellink_core::parameters::transfer::{read_parameters, write_parameters};
use deckellink_core::parameters::{
    diff_parameters, evaluate_parameter_write_gate, parameter_profile, sample_parameters, GateInput,
    ParameterBackupInfo, ParameterCandidate,
};
use deckellink_core::profile::builtin_profiles;
use deckellink_core::transfer::receiver::process_received;
use deckellink_core::transfer::{EndReason, NoEvents, ReceiveState, SendState};
use deckellink_core::transport::lsv2_sim::{Lsv2SimConfig, Lsv2SimulatedCnc};
use deckellink_core::transport::simulated::{SimulatedTransport, SimulationConfig};
use deckellink_core::transport::Transport;

fn profile(id: &str) -> Profile {
    let mut p = builtin_profiles().0.into_iter().find(|p| p.id == id).unwrap();
    p.parameters.allow_write = true;
    p
}

fn plain_cnc(p: &Profile, clock: &FakeClock, program: Vec<u8>) -> SimulatedTransport {
    let config = SimulationConfig { machine_program: program, speed_factor: 50.0, ..SimulationConfig::default() };
    SimulatedTransport::new(p.serial.clone(), config, Arc::new(clock.clone()))
}

fn edited_sample() -> Vec<u8> {
    String::from_utf8(sample_parameters())
        .unwrap()
        .replace("M01001 37", "M01001 38")
        .replace("C001", "C0F1")
        .into_bytes()
}

#[test]
fn plain_read_store_gate_write() {
    let p = profile("dialog11_7n1_rtscts");
    let clock = FakeClock::default();
    let token = CancelToken::new();

    // read: inactivity ends the transfer, no M30 needed, data stored without overwriting
    let mut cnc = plain_cnc(&p, &clock, sample_parameters());
    let result = read_parameters(&mut cnc, &p, &clock, &token, &mut NoEvents);
    assert_eq!(result.state, ReceiveState::Completed, "{:?}", result.error);
    assert!(matches!(result.end_reason, EndReason::Inactivity | EndReason::Eot));
    let program = process_received(&result, &parameter_profile(&p));
    assert!(program.complete, "{:?}", program.notes);
    let dir = std::env::temp_dir().join(format!("deckellink-param-it-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let stored = ParameterArchive::new(&dir).store(&result, &program, &p).unwrap().unwrap();
    assert!(stored.complete());

    // gate with a fresh backup and a properly edited candidate
    let backup = ParameterBackupInfo {
        path_name: "PARAM.par".into(),
        data: program.normalized.clone(),
        read_at: clock.now(),
        profile_id: p.id.clone(),
        port: cnc.name(),
        complete: true,
    };
    let candidate = ParameterCandidate::new("NEU.par", edited_sample(), &p);
    let input = GateInput {
        profile: &p,
        session_unlocked: true,
        candidate: Some(&candidate),
        backup: Some(&backup),
        now: clock.now(),
        port_label: &backup.port,
        simulation: true,
        port: "",
        busy: false,
    };
    let gate = evaluate_parameter_write_gate(&input);
    assert!(gate.allowed, "{:?}", gate.reasons);
    // the same gate without session unlock or with another port stays closed
    assert!(!evaluate_parameter_write_gate(&GateInput { session_unlocked: false, ..input }).allowed);
    assert!(!evaluate_parameter_write_gate(&GateInput { port_label: "COM9", ..input }).allowed);

    // write: exact bytes arrive, nothing added (no M30, no program start)
    let mut target = plain_cnc(&p, &clock, Vec::new());
    let received = target.received_handle();
    let payload = candidate.payload.clone().unwrap();
    let sent = write_parameters(&mut target, &p, &payload, &clock, &token, &mut NoEvents);
    assert_eq!(sent.state, SendState::Completed, "{:?}", sent.error);
    let arrived = received.lock().unwrap().clone();
    assert_eq!(arrived, payload.data);
    assert!(diff_parameters(&edited_sample(), &arrived, "a", "b").identical());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn lsv2_read_and_write() {
    let p = profile("dialog11_lsv2_7e1");
    let clock = FakeClock::default();
    let token = CancelToken::new();
    let config = Lsv2SimConfig { program: sample_parameters(), ..Lsv2SimConfig::default() };
    let mut cnc = Lsv2SimulatedCnc::new(p.serial.clone(), p.protocol.clone(), config, Arc::new(clock.clone()));
    let result = read_parameters(&mut cnc, &p, &clock, &token, &mut NoEvents);
    assert_eq!(result.state, ReceiveState::Completed, "{:?}", result.error);
    // content identical apart from the trailing EOT, which LSV2 transmits as link-level EOT
    let sample = sample_parameters();
    let without_eot = sample.strip_suffix(b"\x04\r\n").unwrap();
    assert_eq!(result.raw, without_eot);

    let candidate = ParameterCandidate::new("NEU.par", edited_sample(), &p);
    let mut target =
        Lsv2SimulatedCnc::new(p.serial.clone(), p.protocol.clone(), Lsv2SimConfig::default(), Arc::new(clock.clone()));
    let sent = write_parameters(&mut target, &p, candidate.payload.as_ref().unwrap(), &clock, &token, &mut NoEvents);
    assert_eq!(sent.state, SendState::Completed, "{:?}", sent.error);
    let arrived = target.record().received_program();
    let edited = edited_sample();
    assert_eq!(arrived, edited.strip_suffix(b"\x04\r\n").unwrap());
}

#[test]
fn locked_profile_blocks_even_with_everything_else() {
    let mut p = profile("dialog11_7n1_rtscts");
    p.parameters.allow_write = false;
    let clock = FakeClock::default();
    let backup = ParameterBackupInfo {
        path_name: "PARAM.par".into(),
        data: sample_parameters(),
        read_at: clock.now(),
        profile_id: p.id.clone(),
        port: "SIM".into(),
        complete: true,
    };
    let candidate = ParameterCandidate::new("NEU.par", edited_sample(), &p);
    let gate = evaluate_parameter_write_gate(&GateInput {
        profile: &p,
        session_unlocked: true,
        candidate: Some(&candidate),
        backup: Some(&backup),
        now: clock.now(),
        port_label: "SIM",
        simulation: true,
        port: "",
        busy: false,
    });
    assert!(!gate.allowed);
    assert_eq!(gate.reasons, vec!["Profil erlaubt kein Schreiben von Maschinenparametern".to_string()]);
}
