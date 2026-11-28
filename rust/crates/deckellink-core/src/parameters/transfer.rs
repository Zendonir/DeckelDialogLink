//! Reading and writing parameter files over the configured protocol (plain or LSV2).

use crate::clock::{CancelToken, Clock};
use crate::model::Profile;
use crate::nc::TransformResult;
use crate::parameters::parameter_profile;
use crate::transfer::lsv2_transfer::{Lsv2ProgramReceiver, Lsv2ProgramSender};
use crate::transfer::receiver::ProgramReceiver;
use crate::transfer::sender::ProgramSender;
use crate::transfer::{EventSink, ReceiveResult, SendResult};
use crate::transport::Transport;

/// Receive the parameter output started by the operator at the CNC.
pub fn read_parameters(
    transport: &mut dyn Transport,
    profile: &Profile,
    clock: &dyn Clock,
    token: &CancelToken,
    events: &mut dyn EventSink,
) -> ReceiveResult {
    let effective = parameter_profile(profile);
    if effective.protocol.is_lsv2() {
        return Lsv2ProgramReceiver::new(transport, &effective, clock, token, events).receive();
    }
    ProgramReceiver::new(transport, &effective, clock, token, events).receive()
}

/// Send a parameter file into the parameter input enabled at the CNC (never drip-feed).
pub fn write_parameters(
    transport: &mut dyn Transport,
    profile: &Profile,
    payload: &TransformResult,
    clock: &dyn Clock,
    token: &CancelToken,
    events: &mut dyn EventSink,
) -> SendResult {
    let effective = parameter_profile(profile);
    if effective.protocol.is_lsv2() {
        let mut sender = Lsv2ProgramSender::new(transport, &effective, clock, token, events, false);
        sender.program_name = "PARAMETER".into();
        return sender.send(payload);
    }
    ProgramSender::new(transport, &effective, clock, token, events).send(payload)
}
