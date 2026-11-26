//! Property tests: arbitrary input must never panic and must keep invariants.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::sync::Arc;

use deckellink_core::clock::{CancelToken, FakeClock};
use deckellink_core::model::{CommentHandling, LineEnding, TabHandling, TransferSettings, WhitespaceHandling};
use deckellink_core::nc::analyzer::analyze;
use deckellink_core::nc::transformer::transform;
use deckellink_core::profile::{builtin_profiles, parse_profile_text};
use deckellink_core::transfer::receiver::{process_received, ProgramReceiver};
use deckellink_core::transfer::sender::ProgramSender;
use deckellink_core::transfer::{NoEvents, SendState};
use deckellink_core::transport::simulated::{SimulatedTransport, SimulationConfig};
use proptest::prelude::*;

fn settings_strategy() -> impl Strategy<Value = TransferSettings> {
    (
        prop::sample::select(vec![LineEnding::Crlf, LineEnding::Lf, LineEnding::Cr]),
        prop::sample::select(vec![TabHandling::Keep, TabHandling::Replace, TabHandling::Reject]),
        prop::sample::select(vec![WhitespaceHandling::None, WhitespaceHandling::Trailing, WhitespaceHandling::Both]),
        prop::sample::select(vec![CommentHandling::Keep, CommentHandling::Remove]),
        prop::sample::select(vec!["", "M30", "M30,M2", "%"]),
        any::<(bool, bool, bool, bool, bool, bool)>(),
    )
        .prop_map(|(line_ending, tab_handling, whitespace, comment_handling, end, flags)| TransferSettings {
            line_ending,
            tab_handling,
            whitespace,
            comment_handling,
            program_end: end.to_string(),
            append_program_end: flags.0,
            remove_blank_lines: flags.1,
            strip_null_bytes: flags.2,
            skip_semicolon_lines: flags.3,
            normalize_line_endings: flags.4,
            send_eot_trailer: flags.5,
            ..TransferSettings::default()
        })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 400, ..ProptestConfig::default() })]

    #[test]
    fn analyze_and_transform_never_panic(data in prop::collection::vec(any::<u8>(), 0..400), settings in settings_strategy()) {
        let analysis = analyze(&data, &settings, None);
        prop_assert_eq!(analysis.byte_count, data.len());
        if let Ok(prepared) = transform(&data, &settings, true) {
            prop_assert_eq!(prepared.line_end_offsets.len(), prepared.line_count);
            prop_assert!(prepared.line_end_offsets.windows(2).all(|w| w[0] <= w[1]));
            prop_assert!(prepared.line_end_offsets.last().map_or(true, |end| *end <= prepared.data.len()));
            prop_assert!(prepared.data.iter().all(|b| *b <= 0x7F));
            if settings.strip_null_bytes {
                prop_assert!(!prepared.data.contains(&0));
            }
        }
    }

    #[test]
    fn profile_parser_never_panics(text in ".{0,300}") {
        let _ = parse_profile_text(&text, "fuzz", false);
    }

    #[test]
    fn profile_parser_with_mutated_values(key in "[a-z_]{1,24}", value in prop::sample::select(vec![
        "null", "true", "-1", "1e309", "4800.5", "\"\"", "[]", "{}", "99999999999999999999", "\"x\""
    ])) {
        let (profiles, _) = builtin_profiles();
        let json = deckellink_core::profile::profile_to_json(&profiles[0]);
        let mut text = json.to_string();
        text = text.replacen(&format!("\"{key}\":"), &format!("\"{key}\":{value},\"_old\":"), 1);
        let _ = parse_profile_text(&text, "fuzz", false);
    }

    #[test]
    fn simulated_roundtrip_delivers_exact_bytes(lines in prop::collection::vec("[A-LN-Z0-9 .]{0,30}", 1..20)) {
        let profile = builtin_profiles().0.into_iter().find(|p| p.id == "dialog11_7e1_none").unwrap();
        let mut program = lines.join("\r\n");
        program.push_str("\r\nN9999 M30\r\n");
        let prepared = transform(program.as_bytes(), &profile.transfer, false).unwrap();
        let clock = FakeClock::default();
        let mut transport = SimulatedTransport::new(profile.serial.clone(), SimulationConfig::default(), Arc::new(clock.clone()));
        let token = CancelToken::new();
        let mut events = NoEvents;
        let result = ProgramSender::new(&mut transport, &profile, &clock, &token, &mut events).send(&prepared);
        prop_assert_eq!(result.state, SendState::Completed);
        prop_assert_eq!(transport.machine_received(), prepared.data.clone());

        // the same bytes sent back by the machine are received completely
        let config = SimulationConfig { machine_program: prepared.data.clone(), ..SimulationConfig::default() };
        let clock = FakeClock::default();
        let mut transport = SimulatedTransport::new(profile.serial.clone(), config, Arc::new(clock.clone()));
        let mut events = NoEvents;
        let received = ProgramReceiver::new(&mut transport, &profile, &clock, &token, &mut events).receive();
        prop_assert!(process_received(&received, &profile).complete);
        prop_assert_eq!(received.raw, prepared.data);
    }
}
