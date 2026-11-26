//! Profile schema validation, serialization, safety warnings and built-in profiles.
//!
//! The JSON format is identical to the Python version; the built-in profile files
//! are compiled in from `src/deckellink/resources/profiles/` (single source of truth).

use regex::Regex;
use serde_json::{Map, Value};

use crate::error::{FieldError, ProfileValidationError};
use crate::model::{
    BccMode, CommentHandling, EndDetection, LineEnding, MachineInfo, ParameterSettings, Parity, Profile, ProtocolMode,
    ProtocolSettings, ReceiveSettings, SerialSettings, StopBits, TabHandling, TransferSettings, VerificationStatus,
    WhitespaceHandling, DATA_BITS, STANDARD_BAUD_RATES, STOP_BITS_HALVES,
};

pub const SCHEMA_VERSION: i64 = 1;
const MAX_BYTES_LIMIT: u64 = 100 * 1024 * 1024;

/// Built-in profiles as (file name, JSON text).
pub const BUILTIN_PROFILES: &[(&str, &str)] = &[
    (
        "dialog11_7n1_rtscts.json",
        include_str!("../../../../src/deckellink/resources/profiles/dialog11_7n1_rtscts.json"),
    ),
    ("dialog11_7e1_none.json", include_str!("../../../../src/deckellink/resources/profiles/dialog11_7e1_none.json")),
    ("dialog11_lsv2_7e1.json", include_str!("../../../../src/deckellink/resources/profiles/dialog11_lsv2_7e1.json")),
];

const KNOWN_TOP_LEVEL: &[&str] = &[
    "schema_version",
    "id",
    "display_name",
    "description",
    "machine",
    "verification_status",
    "serial",
    "transfer",
    "receive",
    "protocol",
    "parameters",
];

/// Python-like `repr` of a JSON value for messages.
pub fn py_repr(value: &Value) -> String {
    match value {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::Number(n) => {
            if n.is_f64() {
                let f = n.as_f64().unwrap_or(0.0);
                if f.fract() == 0.0 && f.abs() < 1e16 {
                    format!("{f:.1}")
                } else {
                    format!("{f}")
                }
            } else {
                n.to_string()
            }
        }
        Value::String(s) => format!("'{s}'"),
        other => other.to_string(),
    }
}

fn is_printable(text: &str) -> bool {
    text.bytes().all(|b| (0x20..=0x7e).contains(&b))
}

fn fmt_num(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value}")
    }
}

/// Reads one section of the profile and records errors/warnings like the Python schema.
struct Section<'a> {
    name: &'a str,
    raw: Option<&'a Map<String, Value>>,
    errors: &'a mut Vec<FieldError>,
    warnings: &'a mut Vec<String>,
}

impl<'a> Section<'a> {
    fn new(
        name: &'a str,
        raw: Option<&'a Value>,
        known: &[&str],
        required: bool,
        errors: &'a mut Vec<FieldError>,
        warnings: &'a mut Vec<String>,
    ) -> (Self, bool) {
        let mut ok = true;
        let map = match raw {
            None => {
                if required {
                    errors.push(FieldError::new(name, "Abschnitt fehlt"));
                    ok = false;
                }
                None
            }
            Some(Value::Object(map)) => {
                for key in map.keys() {
                    if !known.contains(&key.as_str()) {
                        warnings.push(format!("Unbekanntes Feld '{name}.{key}' wird ignoriert."));
                    }
                }
                Some(map)
            }
            Some(_) => {
                errors.push(FieldError::new(name, "Objekt erwartet"));
                ok = false;
                None
            }
        };
        (Self { name, raw: map, errors, warnings }, ok)
    }

    fn path(&self, field: &str) -> String {
        format!("{}.{}", self.name, field)
    }

    fn get(&mut self, field: &str, default_repr: &str) -> Option<&'a Value> {
        let map = self.raw?;
        match map.get(field) {
            Some(value) => Some(value),
            None => {
                let path = self.path(field);
                self.warnings.push(format!("Feld '{path}' fehlt - Standardwert {default_repr} wird verwendet."));
                None
            }
        }
    }

    fn error(&mut self, field: &str, message: String) {
        let path = self.path(field);
        self.errors.push(FieldError::new(path, message));
    }

    fn boolean(&mut self, field: &str, default: bool) -> bool {
        let repr = if default { "True" } else { "False" };
        match self.get(field, repr) {
            None => default,
            Some(Value::Bool(b)) => *b,
            Some(other) => {
                let msg = format!("Wahrheitswert (true/false) erwartet, erhalten {}", py_repr(other));
                self.error(field, msg);
                default
            }
        }
    }

    fn integer(&mut self, field: &str, default: u64, min: i64, max: i64) -> u64 {
        let Some(value) = self.get(field, &default.to_string()) else {
            return default;
        };
        let Some(number) = value.as_i64().filter(|_| value.is_i64() || value.is_u64()) else {
            let msg = format!("Ganzzahl erwartet, erhalten {}", py_repr(value));
            self.error(field, msg);
            return default;
        };
        if number < min {
            self.error(field, format!("Wert {number} kleiner als Minimum {min}"));
            return default;
        }
        if number > max {
            self.error(field, format!("Wert {number} größer als Maximum {max}"));
            return default;
        }
        u64::try_from(number).unwrap_or(default)
    }

    fn int_choice(&mut self, field: &str, default: u64, choices: &[i64]) -> u64 {
        let Some(value) = self.get(field, &default.to_string()) else {
            return default;
        };
        let Some(number) = value.as_i64().filter(|_| value.is_i64() || value.is_u64()) else {
            let msg = format!("Ganzzahl erwartet, erhalten {}", py_repr(value));
            self.error(field, msg);
            return default;
        };
        if !choices.contains(&number) {
            let shown = choices.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ");
            self.error(field, format!("ungültiger Wert {number}, erlaubt: {shown}"));
            return default;
        }
        u64::try_from(number).unwrap_or(default)
    }

    fn stop_bits(&mut self, field: &str, default: StopBits) -> StopBits {
        let Some(value) = self.get(field, default.label()) else {
            return default;
        };
        let Some(number) = value.as_f64().filter(|_| value.is_number()) else {
            let msg = format!("Zahl erwartet, erhalten {}", py_repr(value));
            self.error(field, msg);
            return default;
        };
        let halves = (number * 2.0).round();
        let valid = (number * 2.0 - halves).abs() < 1e-9;
        #[allow(clippy::cast_possible_truncation)]
        let parsed = if valid && halves.abs() < 1e6 { StopBits::from_halves(halves as i64) } else { None };
        match parsed {
            Some(bits) if STOP_BITS_HALVES.contains(&((bits.as_f64() * 2.0) as i64)) => bits,
            _ => {
                self.error(field, format!("ungültiger Wert {}, erlaubt: 1.0, 1.5, 2.0", fmt_num(number)));
                default
            }
        }
    }

    fn string(&mut self, field: &str, default: &str, max_len: Option<usize>, printable: bool) -> String {
        let Some(value) = self.get(field, &format!("'{default}'")) else {
            return default.to_string();
        };
        let Some(text) = value.as_str() else {
            let msg = format!("Text erwartet, erhalten {}", py_repr(value));
            self.error(field, msg);
            return default.to_string();
        };
        if let Some(max) = max_len {
            if text.chars().count() > max {
                self.error(field, format!("Text zu lang (max. {max} Zeichen)"));
                return default.to_string();
            }
        }
        if printable && !is_printable(text) {
            self.error(field, "nur druckbare ASCII-Zeichen erlaubt".into());
            return default.to_string();
        }
        text.to_string()
    }

    fn enumeration<T: Copy>(
        &mut self,
        field: &str,
        default: T,
        default_text: &str,
        parse: fn(&str) -> Option<T>,
        allowed: &str,
    ) -> T {
        let Some(value) = self.get(field, &format!("<{default_text}>")) else {
            return default;
        };
        match value.as_str().and_then(parse) {
            Some(parsed) => parsed,
            None => {
                let msg = format!("ungültiger Wert {}, erlaubt: {allowed}", py_repr(value));
                self.error(field, msg);
                default
            }
        }
    }

    fn string_list(&mut self, field: &str, max_items: usize, max_len: usize) -> Vec<String> {
        let Some(value) = self.get(field, "()") else {
            return Vec::new();
        };
        let Some(items) = value.as_array() else {
            self.error(field, "Liste von Texten erwartet".into());
            return Vec::new();
        };
        let mut out = Vec::new();
        for item in items {
            let Some(text) = item.as_str() else {
                self.error(field, "Liste von Texten erwartet".into());
                return Vec::new();
            };
            out.push(text.to_string());
        }
        if out.len() > max_items {
            self.error(field, format!("höchstens {max_items} Einträge erlaubt"));
            return Vec::new();
        }
        for text in &out {
            if text.chars().count() > max_len || !is_printable(text) {
                self.error(field, format!("Eintrag '{text}': nur druckbare ASCII-Zeichen, max. {max_len}"));
                return Vec::new();
            }
        }
        out
    }
}

const SERIAL_FIELDS: &[&str] = &[
    "baud_rate",
    "data_bits",
    "parity",
    "stop_bits",
    "rts_cts",
    "xon_xoff",
    "dsr_dtr",
    "initial_rts",
    "initial_dtr",
    "require_cts_before_send",
    "require_dsr_before_send",
    "read_timeout_ms",
    "write_timeout_ms",
    "inter_byte_timeout_ms",
    "handshake_timeout_ms",
];
const TRANSFER_FIELDS: &[&str] = &[
    "encoding",
    "line_ending",
    "program_start",
    "program_end",
    "append_program_end",
    "start_delay_ms",
    "end_delay_ms",
    "character_delay_ms",
    "packet_size",
    "packet_delay_ms",
    "remove_blank_lines",
    "strip_null_bytes",
    "skip_semicolon_lines",
    "normalize_line_endings",
    "tab_handling",
    "whitespace",
    "comment_handling",
    "comment_open",
    "comment_close",
    "ensure_final_newline",
    "send_eot_trailer",
    "max_line_length",
    "max_program_bytes",
    "close_port_after_send",
];
const RECEIVE_FIELDS: &[&str] = &[
    "end_detection",
    "end_sequence",
    "inactivity_timeout_ms",
    "first_byte_timeout_ms",
    "end_grace_ms",
    "max_receive_bytes",
    "strip_null_bytes",
    "normalize_line_endings",
    "save_raw",
    "end_on_eot",
    "program_name_pattern",
];
const PROTOCOL_FIELDS: &[&str] = &[
    "mode",
    "transparent",
    "bcc_mode",
    "enq_interval_ms",
    "link_timeout_ms",
    "ack_timeout_ms",
    "drip_feed_ack_timeout_ms",
    "max_retries",
    "wack_poll_ms",
    "lines_per_telegram",
    "max_telegram_length",
    "include_line_ending",
    "start_telegrams",
    "end_telegrams",
    "send_eot_on_cancel",
];
const PARAMETER_FIELDS: &[&str] = &[
    "allow_write",
    "inactivity_timeout_ms",
    "first_byte_timeout_ms",
    "max_bytes",
    "identification_pattern",
    "max_backup_age_min",
];

#[allow(clippy::cast_possible_wrap)]
const LIMIT: i64 = MAX_BYTES_LIMIT as i64;

fn parse_serial(s: &mut Section<'_>) -> SerialSettings {
    let d = SerialSettings::default();
    #[allow(clippy::cast_possible_truncation)]
    SerialSettings {
        baud_rate: s.int_choice("baud_rate", u64::from(d.baud_rate), STANDARD_BAUD_RATES) as u32,
        data_bits: s.int_choice("data_bits", u64::from(d.data_bits), DATA_BITS) as u8,
        parity: s.enumeration("parity", d.parity, "Parity.NONE", Parity::parse, &Parity::allowed()),
        stop_bits: s.stop_bits("stop_bits", d.stop_bits),
        rts_cts: s.boolean("rts_cts", d.rts_cts),
        xon_xoff: s.boolean("xon_xoff", d.xon_xoff),
        dsr_dtr: s.boolean("dsr_dtr", d.dsr_dtr),
        initial_rts: s.boolean("initial_rts", d.initial_rts),
        initial_dtr: s.boolean("initial_dtr", d.initial_dtr),
        require_cts_before_send: s.boolean("require_cts_before_send", d.require_cts_before_send),
        require_dsr_before_send: s.boolean("require_dsr_before_send", d.require_dsr_before_send),
        read_timeout_ms: s.integer("read_timeout_ms", d.read_timeout_ms, 1, 600_000),
        write_timeout_ms: s.integer("write_timeout_ms", d.write_timeout_ms, 1, 600_000),
        inter_byte_timeout_ms: s.integer("inter_byte_timeout_ms", d.inter_byte_timeout_ms, 0, 60_000),
        handshake_timeout_ms: s.integer("handshake_timeout_ms", d.handshake_timeout_ms, 0, 3_600_000),
    }
}

fn parse_transfer(s: &mut Section<'_>) -> TransferSettings {
    let d = TransferSettings::default();
    let encoding = s.string("encoding", &d.encoding, None, false);
    let encoding = if encoding == "ascii" {
        encoding
    } else {
        s.error("encoding", format!("ungültiger Wert '{encoding}', erlaubt: ascii"));
        d.encoding.clone()
    };
    TransferSettings {
        encoding,
        line_ending: s.enumeration(
            "line_ending",
            d.line_ending,
            "LineEnding.CRLF",
            LineEnding::parse,
            &LineEnding::allowed(),
        ),
        program_start: s.string("program_start", &d.program_start, Some(64), true),
        program_end: s.string("program_end", &d.program_end, Some(32), true),
        append_program_end: s.boolean("append_program_end", d.append_program_end),
        start_delay_ms: s.integer("start_delay_ms", d.start_delay_ms, 0, 600_000),
        end_delay_ms: s.integer("end_delay_ms", d.end_delay_ms, 0, 600_000),
        character_delay_ms: s.integer("character_delay_ms", d.character_delay_ms, 0, 10_000),
        packet_size: s.integer("packet_size", d.packet_size, 0, 65_536),
        packet_delay_ms: s.integer("packet_delay_ms", d.packet_delay_ms, 0, 600_000),
        remove_blank_lines: s.boolean("remove_blank_lines", d.remove_blank_lines),
        strip_null_bytes: s.boolean("strip_null_bytes", d.strip_null_bytes),
        skip_semicolon_lines: s.boolean("skip_semicolon_lines", d.skip_semicolon_lines),
        normalize_line_endings: s.boolean("normalize_line_endings", d.normalize_line_endings),
        tab_handling: s.enumeration(
            "tab_handling",
            d.tab_handling,
            "TabHandling.REPLACE",
            TabHandling::parse,
            &TabHandling::allowed(),
        ),
        whitespace: s.enumeration(
            "whitespace",
            d.whitespace,
            "WhitespaceHandling.TRAILING",
            WhitespaceHandling::parse,
            &WhitespaceHandling::allowed(),
        ),
        comment_handling: s.enumeration(
            "comment_handling",
            d.comment_handling,
            "CommentHandling.KEEP",
            CommentHandling::parse,
            &CommentHandling::allowed(),
        ),
        comment_open: s.string("comment_open", &d.comment_open, Some(2), true),
        comment_close: s.string("comment_close", &d.comment_close, Some(2), true),
        ensure_final_newline: s.boolean("ensure_final_newline", d.ensure_final_newline),
        send_eot_trailer: s.boolean("send_eot_trailer", d.send_eot_trailer),
        max_line_length: s.integer("max_line_length", d.max_line_length, 0, 100_000),
        max_program_bytes: s.integer("max_program_bytes", d.max_program_bytes, 0, LIMIT),
        close_port_after_send: s.boolean("close_port_after_send", d.close_port_after_send),
    }
}

fn parse_receive(s: &mut Section<'_>) -> ReceiveSettings {
    let d = ReceiveSettings::default();
    ReceiveSettings {
        end_detection: s.enumeration(
            "end_detection",
            d.end_detection,
            "EndDetection.BOTH",
            EndDetection::parse,
            &EndDetection::allowed(),
        ),
        end_sequence: s.string("end_sequence", &d.end_sequence, Some(32), true),
        inactivity_timeout_ms: s.integer("inactivity_timeout_ms", d.inactivity_timeout_ms, 50, 3_600_000),
        first_byte_timeout_ms: s.integer("first_byte_timeout_ms", d.first_byte_timeout_ms, 0, 86_400_000),
        end_grace_ms: s.integer("end_grace_ms", d.end_grace_ms, 0, 60_000),
        max_receive_bytes: s.integer("max_receive_bytes", d.max_receive_bytes, 0, LIMIT),
        strip_null_bytes: s.boolean("strip_null_bytes", d.strip_null_bytes),
        normalize_line_endings: s.boolean("normalize_line_endings", d.normalize_line_endings),
        save_raw: s.boolean("save_raw", d.save_raw),
        end_on_eot: s.boolean("end_on_eot", d.end_on_eot),
        program_name_pattern: s.string("program_name_pattern", &d.program_name_pattern, Some(200), false),
    }
}

fn parse_protocol(s: &mut Section<'_>) -> ProtocolSettings {
    let d = ProtocolSettings::default();
    ProtocolSettings {
        mode: s.enumeration("mode", d.mode, "ProtocolMode.PLAIN", ProtocolMode::parse, &ProtocolMode::allowed()),
        transparent: s.boolean("transparent", d.transparent),
        bcc_mode: s.enumeration("bcc_mode", d.bcc_mode, "BccMode.TEXT_ETX", BccMode::parse, &BccMode::allowed()),
        enq_interval_ms: s.integer("enq_interval_ms", d.enq_interval_ms, 100, 60_000),
        link_timeout_ms: s.integer("link_timeout_ms", d.link_timeout_ms, 100, 3_600_000),
        ack_timeout_ms: s.integer("ack_timeout_ms", d.ack_timeout_ms, 50, 600_000),
        drip_feed_ack_timeout_ms: s.integer("drip_feed_ack_timeout_ms", d.drip_feed_ack_timeout_ms, 0, 86_400_000),
        max_retries: s.integer("max_retries", d.max_retries, 0, 20),
        wack_poll_ms: s.integer("wack_poll_ms", d.wack_poll_ms, 10, 60_000),
        lines_per_telegram: s.integer("lines_per_telegram", d.lines_per_telegram, 1, 1000),
        max_telegram_length: s.integer("max_telegram_length", d.max_telegram_length, 8, 4096),
        include_line_ending: s.boolean("include_line_ending", d.include_line_ending),
        start_telegrams: s.string_list("start_telegrams", 10, 128),
        end_telegrams: s.string_list("end_telegrams", 10, 128),
        send_eot_on_cancel: s.boolean("send_eot_on_cancel", d.send_eot_on_cancel),
    }
}

fn parse_parameters(s: &mut Section<'_>) -> ParameterSettings {
    let d = ParameterSettings::default();
    ParameterSettings {
        allow_write: s.boolean("allow_write", d.allow_write),
        inactivity_timeout_ms: s.integer("inactivity_timeout_ms", d.inactivity_timeout_ms, 200, 3_600_000),
        first_byte_timeout_ms: s.integer("first_byte_timeout_ms", d.first_byte_timeout_ms, 0, 86_400_000),
        max_bytes: s.integer("max_bytes", d.max_bytes, 1, LIMIT),
        identification_pattern: s.string("identification_pattern", &d.identification_pattern, Some(200), false),
        max_backup_age_min: s.integer("max_backup_age_min", d.max_backup_age_min, 1, 24 * 60),
    }
}

/// Translate a Python `re` pattern to the Rust `regex` dialect where they differ (inline flags are identical).
pub fn compile_pattern(pattern: &str) -> Result<Regex, regex::Error> {
    Regex::new(pattern)
}

/// Validate `data` and build a [`Profile`]; returns non-fatal warnings (structure and safety).
pub fn parse_profile(
    data: &Value,
    source: &str,
    builtin: bool,
) -> Result<(Profile, Vec<String>), ProfileValidationError> {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let Some(map) = data.as_object() else {
        return Err(ProfileValidationError {
            errors: vec![FieldError::new("(Datei)", "JSON-Objekt erwartet")],
            source_name: source.to_string(),
        });
    };
    for key in map.keys() {
        if !KNOWN_TOP_LEVEL.contains(&key.as_str()) {
            warnings.push(format!("Unbekanntes Feld '{key}' wird ignoriert."));
        }
    }
    let version = map.get("schema_version");
    if version.and_then(Value::as_i64) != Some(SCHEMA_VERSION) || version.is_some_and(Value::is_f64) {
        let shown = version.map_or_else(|| "None".to_string(), py_repr);
        errors.push(FieldError::new("schema_version", format!("Version {SCHEMA_VERSION} erwartet, erhalten {shown}")));
    }
    let id = map.get("id").and_then(Value::as_str).unwrap_or("");
    if !valid_profile_id(id) {
        errors.push(FieldError::new("id", "Kleinbuchstaben, Ziffern, '_' oder '-' (max. 64 Zeichen) erwartet"));
    }
    let display_name = map.get("display_name").and_then(Value::as_str).unwrap_or("");
    if display_name.trim().is_empty() || display_name.chars().count() > 120 {
        errors.push(FieldError::new("display_name", "nicht-leerer Text (max. 120 Zeichen) erwartet"));
    }
    let description = match map.get("description") {
        None => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(_) => {
            errors.push(FieldError::new("description", "Text erwartet"));
            String::new()
        }
    };
    let mut machine = MachineInfo::default();
    match map.get("machine") {
        None => {}
        Some(Value::Object(raw)) => {
            let text = |key: &str, default: &str| match raw.get(key) {
                Some(Value::String(s)) => s.clone(),
                Some(other) => other.to_string(),
                None => default.to_string(),
            };
            machine = MachineInfo {
                manufacturer: text("manufacturer", &machine.manufacturer),
                model: text("model", &machine.model),
                control: text("control", &machine.control),
            };
        }
        Some(_) => errors.push(FieldError::new("machine", "Objekt erwartet")),
    }
    let status = match map.get("verification_status") {
        None => VerificationStatus::Unverified,
        Some(Value::String(s)) if s == "unverified" => VerificationStatus::Unverified,
        Some(Value::String(s)) if s == "verified" => VerificationStatus::Verified,
        Some(_) => {
            errors.push(FieldError::new("verification_status", "erwartet 'unverified' oder 'verified'"));
            VerificationStatus::Unverified
        }
    };

    let serial = {
        let (mut s, _) = Section::new("serial", map.get("serial"), SERIAL_FIELDS, true, &mut errors, &mut warnings);
        parse_serial(&mut s)
    };
    let transfer = {
        let (mut s, _) =
            Section::new("transfer", map.get("transfer"), TRANSFER_FIELDS, true, &mut errors, &mut warnings);
        parse_transfer(&mut s)
    };
    let receive = {
        let (mut s, _) = Section::new("receive", map.get("receive"), RECEIVE_FIELDS, false, &mut errors, &mut warnings);
        parse_receive(&mut s)
    };
    let protocol = {
        let (mut s, _) =
            Section::new("protocol", map.get("protocol"), PROTOCOL_FIELDS, false, &mut errors, &mut warnings);
        parse_protocol(&mut s)
    };
    let parameters = {
        let (mut s, _) =
            Section::new("parameters", map.get("parameters"), PARAMETER_FIELDS, false, &mut errors, &mut warnings);
        parse_parameters(&mut s)
    };

    check_receive_semantics(&receive, &mut errors);
    if !parameters.identification_pattern.is_empty() {
        if let Err(exc) = compile_pattern(&parameters.identification_pattern) {
            errors.push(FieldError::new(
                "parameters.identification_pattern",
                format!("ungültiger regulärer Ausdruck: {exc}"),
            ));
        }
    }
    if !errors.is_empty() {
        return Err(ProfileValidationError { errors, source_name: source.to_string() });
    }
    let profile = Profile {
        id: id.to_string(),
        display_name: display_name.trim().to_string(),
        machine,
        verification_status: status,
        serial,
        transfer,
        receive,
        protocol,
        parameters,
        description,
        schema_version: 1,
        builtin,
    };
    warnings.extend(safety_warnings(&profile));
    Ok((profile, warnings))
}

/// Profile ids: lowercase letters, digits, '_' or '-', 1..64 characters, starting with letter/digit.
pub fn valid_profile_id(id: &str) -> bool {
    let mut chars = id.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    id.len() <= 64
        && (first.is_ascii_lowercase() || first.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

fn check_receive_semantics(receive: &ReceiveSettings, errors: &mut Vec<FieldError>) {
    if receive.end_detection.uses_end_sequence() && receive.end_sequence.trim().is_empty() {
        errors.push(FieldError::new("receive.end_sequence", "darf nicht leer sein, wenn auf Endsequenz geprüft wird"));
    }
    if !receive.program_name_pattern.is_empty() {
        match compile_pattern(&receive.program_name_pattern) {
            Err(exc) => errors
                .push(FieldError::new("receive.program_name_pattern", format!("ungültiger regulärer Ausdruck: {exc}"))),
            Ok(regex) => {
                if regex.captures_len() < 2 {
                    errors.push(FieldError::new(
                        "receive.program_name_pattern",
                        "benötigt eine Gruppe (...) für den Namen",
                    ));
                }
            }
        }
    }
}

/// Warnings about unsafe or questionable combinations (never silently accepted).
pub fn safety_warnings(profile: &Profile) -> Vec<String> {
    let s = &profile.serial;
    let t = &profile.transfer;
    let r = &profile.receive;
    let mut out = Vec::new();
    if s.data_bits < 7 {
        out.push(format!("{} Datenbits: 7-Bit-ASCII-Zeichen sind damit nicht vollständig darstellbar.", s.data_bits));
    }
    if s.rts_cts && !s.initial_rts {
        out.push(
            "RTS/CTS ist aktiv, gleichzeitig ist RTS manuell auf 'aus' gesetzt. Bei Hardware-Handshake steuert \
             der Treiber RTS; ein erzwungener Zustand kann die Gegenstelle blockieren."
                .into(),
        );
    }
    if [s.rts_cts, s.xon_xoff, s.dsr_dtr].iter().filter(|b| **b).count() > 1 {
        out.push("Mehrere Flusssteuerungen gleichzeitig aktiv - ungewöhnliche Kombination.".into());
    }
    if s.require_cts_before_send && !s.rts_cts {
        out.push("CTS wird vor Sendebeginn geprüft, aber nicht während der Übertragung (kein RTS/CTS).".into());
    }
    if r.max_receive_bytes == 0 {
        out.push("Unbegrenzter Empfang: Es ist keine maximale Empfangsgröße gesetzt.".into());
    }
    let mut short = Vec::new();
    if s.read_timeout_ms < 100 {
        short.push(format!("Read Timeout {} ms", s.read_timeout_ms));
    }
    if s.write_timeout_ms < 500 {
        short.push(format!("Write Timeout {} ms", s.write_timeout_ms));
    }
    if s.handshake_timeout_ms > 0 && s.handshake_timeout_ms < 1000 {
        short.push(format!("Handshake-Wartezeit {} ms", s.handshake_timeout_ms));
    }
    if r.inactivity_timeout_ms < 1000 {
        short.push(format!("Empfangs-Inaktivität {} ms", r.inactivity_timeout_ms));
    }
    if !short.is_empty() {
        out.push(format!("Extrem kurze Timeouts: {}.", short.join(", ")));
    }
    if t.append_program_end {
        out.push(format!(
            "Automatisches Ergänzen von '{}' ist aktiviert. Es erfolgt nur nach ausdrücklicher Bestätigung pro \
             Übertragung.",
            t.program_end
        ));
    }
    if !t.program_start.trim().is_empty() {
        out.push(format!(
            "Startsequenz '{}' ist gesetzt. Ihre Bedeutung für die DIALOG 11 ist nicht verifiziert (z. B. M82).",
            t.program_start.trim()
        ));
    }
    if t.tab_handling == TabHandling::Keep {
        out.push("Tabulatoren werden unverändert gesendet - Verhalten der Steuerung unbekannt.".into());
    }
    if t.program_end.trim().is_empty() {
        out.push("Kein Programmende definiert - fehlendes Programmende wird nicht erkannt.".into());
    }
    if profile.parameters.allow_write {
        out.push(
            "Schreiben von Maschinenparametern ist in diesem Profil erlaubt. Falsche Parameter können Maschine, \
             Werkstück und Personen gefährden - das Parameterformat der DIALOG 11 ist nicht verifiziert."
                .into(),
        );
    }
    let p = &profile.protocol;
    if p.is_lsv2() {
        out.push(
            "LSV2-Protokoll aktiv: Telegrammaufbau (DLE-Transparenz, BCC, Quittungen) ist für die DIALOG 11 nicht \
             verifiziert."
                .into(),
        );
        if p.drip_feed_ack_timeout_ms == 0 {
            out.push("Nachladebetrieb ohne Zeitlimit für Quittungen der CNC (wartet unbegrenzt).".into());
        }
        if !p.start_telegrams.is_empty() || !p.end_telegrams.is_empty() {
            out.push("Start-/Endtelegramme sind gesetzt - ihr Inhalt ist für die DIALOG 11 nicht verifiziert.".into());
        }
        if p.max_retries == 0 {
            out.push("LSV2 ohne Wiederholungen: jeder Übertragungsfehler bricht ab.".into());
        }
        if !p.include_line_ending {
            out.push("Telegramme ohne Zeilenende - Zeilentrennung erfolgt nur über die Telegrammgrenzen.".into());
        }
    }
    out
}

/// Serialize to the JSON profile format (`builtin` is not persisted).
pub fn profile_to_json(profile: &Profile) -> Value {
    let s = &profile.serial;
    let t = &profile.transfer;
    let r = &profile.receive;
    let p = &profile.protocol;
    let q = &profile.parameters;
    let stop_bits: Value = match s.stop_bits {
        StopBits::One => 1.into(),
        StopBits::OnePointFive => 1.5.into(),
        StopBits::Two => 2.into(),
    };
    serde_json::json!({
        "schema_version": profile.schema_version,
        "id": profile.id,
        "display_name": profile.display_name,
        "description": profile.description,
        "machine": {
            "manufacturer": profile.machine.manufacturer,
            "model": profile.machine.model,
            "control": profile.machine.control,
        },
        "verification_status": profile.verification_status.as_str(),
        "serial": {
            "baud_rate": s.baud_rate, "data_bits": s.data_bits, "parity": s.parity.as_str(),
            "stop_bits": stop_bits, "rts_cts": s.rts_cts, "xon_xoff": s.xon_xoff, "dsr_dtr": s.dsr_dtr,
            "initial_rts": s.initial_rts, "initial_dtr": s.initial_dtr,
            "require_cts_before_send": s.require_cts_before_send,
            "require_dsr_before_send": s.require_dsr_before_send,
            "read_timeout_ms": s.read_timeout_ms, "write_timeout_ms": s.write_timeout_ms,
            "inter_byte_timeout_ms": s.inter_byte_timeout_ms, "handshake_timeout_ms": s.handshake_timeout_ms,
        },
        "transfer": {
            "encoding": t.encoding, "line_ending": t.line_ending.as_str(), "program_start": t.program_start,
            "program_end": t.program_end, "append_program_end": t.append_program_end,
            "start_delay_ms": t.start_delay_ms, "end_delay_ms": t.end_delay_ms,
            "character_delay_ms": t.character_delay_ms, "packet_size": t.packet_size,
            "packet_delay_ms": t.packet_delay_ms, "remove_blank_lines": t.remove_blank_lines,
            "strip_null_bytes": t.strip_null_bytes, "skip_semicolon_lines": t.skip_semicolon_lines,
            "normalize_line_endings": t.normalize_line_endings, "tab_handling": t.tab_handling.as_str(),
            "whitespace": t.whitespace.as_str(), "comment_handling": t.comment_handling.as_str(),
            "comment_open": t.comment_open, "comment_close": t.comment_close,
            "ensure_final_newline": t.ensure_final_newline, "send_eot_trailer": t.send_eot_trailer,
            "max_line_length": t.max_line_length, "max_program_bytes": t.max_program_bytes,
            "close_port_after_send": t.close_port_after_send,
        },
        "receive": {
            "end_detection": r.end_detection.as_str(), "end_sequence": r.end_sequence,
            "inactivity_timeout_ms": r.inactivity_timeout_ms, "first_byte_timeout_ms": r.first_byte_timeout_ms,
            "end_grace_ms": r.end_grace_ms, "max_receive_bytes": r.max_receive_bytes,
            "strip_null_bytes": r.strip_null_bytes, "normalize_line_endings": r.normalize_line_endings,
            "save_raw": r.save_raw, "end_on_eot": r.end_on_eot, "program_name_pattern": r.program_name_pattern,
        },
        "protocol": {
            "mode": p.mode.as_str(), "transparent": p.transparent, "bcc_mode": p.bcc_mode.as_str(),
            "enq_interval_ms": p.enq_interval_ms, "link_timeout_ms": p.link_timeout_ms,
            "ack_timeout_ms": p.ack_timeout_ms, "drip_feed_ack_timeout_ms": p.drip_feed_ack_timeout_ms,
            "max_retries": p.max_retries, "wack_poll_ms": p.wack_poll_ms,
            "lines_per_telegram": p.lines_per_telegram, "max_telegram_length": p.max_telegram_length,
            "include_line_ending": p.include_line_ending, "start_telegrams": p.start_telegrams,
            "end_telegrams": p.end_telegrams, "send_eot_on_cancel": p.send_eot_on_cancel,
        },
        "parameters": {
            "allow_write": q.allow_write, "inactivity_timeout_ms": q.inactivity_timeout_ms,
            "first_byte_timeout_ms": q.first_byte_timeout_ms, "max_bytes": q.max_bytes,
            "identification_pattern": q.identification_pattern, "max_backup_age_min": q.max_backup_age_min,
        },
    })
}

/// Parse a profile from JSON text.
pub fn parse_profile_text(
    text: &str,
    source: &str,
    builtin: bool,
) -> Result<(Profile, Vec<String>), ProfileValidationError> {
    let value: Value = serde_json::from_str(text).map_err(|exc| ProfileValidationError {
        errors: vec![FieldError::new("(Datei)", format!("Ungültiges JSON: {exc}"))],
        source_name: source.to_string(),
    })?;
    parse_profile(&value, source, builtin)
}

/// All built-in profiles; profiles that fail validation are reported, never panic.
pub fn builtin_profiles() -> (Vec<Profile>, Vec<ProfileValidationError>) {
    let mut profiles = Vec::new();
    let mut problems = Vec::new();
    for (name, text) in BUILTIN_PROFILES {
        match parse_profile_text(text, name, true) {
            Ok((profile, _)) => profiles.push(profile),
            Err(err) => problems.push(err),
        }
    }
    (profiles, problems)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn builtin(name: &str) -> Value {
        let text = BUILTIN_PROFILES.iter().find(|(n, _)| n.starts_with(name)).map(|(_, t)| *t).unwrap();
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn builtin_profiles_load() {
        let (profiles, problems) = builtin_profiles();
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(profiles.len(), 3);
        let a = profiles.iter().find(|p| p.id == "dialog11_7n1_rtscts").unwrap();
        assert_eq!(a.serial.summary(), "4800 Baud | 7N1 | RTS/CTS");
        assert_eq!(a.transfer.program_end, "M30,M2");
        assert!(a.receive.end_on_eot && a.transfer.send_eot_trailer && !a.parameters.allow_write);
        let c = profiles.iter().find(|p| p.id == "dialog11_lsv2_7e1").unwrap();
        assert!(c.protocol.is_lsv2());
        assert!(profiles.iter().all(|p| p.verification_status == VerificationStatus::Unverified));
    }

    #[test]
    fn roundtrip() {
        let (profile, warnings) = parse_profile(&builtin("dialog11_7n1"), "", false).unwrap();
        assert!(!warnings.iter().any(|w| w.contains("fehlt")));
        let (again, _) = parse_profile(&profile_to_json(&profile), "", false).unwrap();
        assert_eq!(again, profile);
    }

    #[test]
    fn invalid_fields_rejected() {
        let cases = [
            ("serial", "parity", json!("evenish"), "serial.parity"),
            ("serial", "baud_rate", json!(4801), "serial.baud_rate"),
            ("serial", "data_bits", json!(9), "serial.data_bits"),
            ("serial", "stop_bits", json!(3), "serial.stop_bits"),
            ("serial", "rts_cts", json!("yes"), "serial.rts_cts"),
            ("serial", "read_timeout_ms", json!(0), "serial.read_timeout_ms"),
            ("serial", "read_timeout_ms", json!(10.0), "serial.read_timeout_ms"),
            ("transfer", "encoding", json!("utf-8"), "transfer.encoding"),
            ("transfer", "program_end", json!("M30ä"), "transfer.program_end"),
            ("transfer", "start_delay_ms", json!(true), "transfer.start_delay_ms"),
            ("protocol", "start_telegrams", json!(["ok", 5]), "protocol.start_telegrams"),
            ("parameters", "identification_pattern", json!("(broken"), "parameters.identification_pattern"),
        ];
        for (section, key, value, field) in cases {
            let mut data = builtin("dialog11_7n1");
            data[section][key] = value;
            let err = parse_profile(&data, "", false).unwrap_err();
            assert!(err.errors.iter().any(|e| e.field == field), "{field}: {err}");
        }
    }

    #[test]
    fn multiple_errors_and_structure() {
        let mut data = builtin("dialog11_7n1");
        data["serial"]["parity"] = json!("x");
        data["serial"]["baud_rate"] = json!(1);
        data["id"] = json!("Invalid ID!");
        let err = parse_profile(&data, "", false).unwrap_err();
        let fields: Vec<_> = err.errors.iter().map(|e| e.field.as_str()).collect();
        assert!(fields.contains(&"serial.parity") && fields.contains(&"serial.baud_rate") && fields.contains(&"id"));
        let mut data = builtin("dialog11_7n1");
        data.as_object_mut().unwrap().remove("serial");
        data["schema_version"] = json!(2);
        let err = parse_profile(&data, "", false).unwrap_err();
        let fields: Vec<_> = err.errors.iter().map(|e| e.field.as_str()).collect();
        assert!(fields.contains(&"serial") && fields.contains(&"schema_version"));
        assert!(parse_profile(&json!(["x"]), "", false).is_err());
        assert!(parse_profile_text("{ nicht json", "x", false).unwrap_err().to_string().contains("Ungültiges JSON"));
    }

    #[test]
    fn missing_and_unknown_fields_warn() {
        let mut data = builtin("dialog11_7n1");
        data["serial"].as_object_mut().unwrap().remove("inter_byte_timeout_ms");
        data["serial"]["magic"] = json!(1);
        data["extra"] = json!(true);
        data.as_object_mut().unwrap().remove("receive");
        let (profile, warnings) = parse_profile(&data, "", false).unwrap();
        assert_eq!(profile.serial.inter_byte_timeout_ms, 0);
        assert!(warnings.iter().any(|w| w.contains("inter_byte_timeout_ms") && w.contains("fehlt")));
        assert!(warnings.iter().any(|w| w.contains("serial.magic")));
        assert!(warnings.iter().any(|w| w.contains("'extra'")));
        assert_eq!(profile.receive.end_sequence, "M30");
    }

    #[test]
    fn receive_semantics() {
        let mut data = builtin("dialog11_7n1");
        data["receive"]["end_sequence"] = json!("");
        assert!(parse_profile(&data, "", false).is_err());
        let mut data = builtin("dialog11_7n1");
        data["receive"]["program_name_pattern"] = json!("(unclosed");
        assert!(parse_profile(&data, "", false).is_err());
        data["receive"]["program_name_pattern"] = json!("O\\d+");
        assert!(parse_profile(&data, "", false).is_err());
        data["receive"]["program_name_pattern"] = json!("O(\\d+)");
        assert!(parse_profile(&data, "", false).is_ok());
    }

    #[test]
    fn safety_warning_texts() {
        let (profile, _) = parse_profile(&builtin("dialog11_7n1"), "", false).unwrap();
        assert!(safety_warnings(&profile).is_empty());
        let mut risky = profile;
        risky.serial.data_bits = 6;
        risky.serial.initial_rts = false;
        risky.serial.read_timeout_ms = 10;
        risky.serial.xon_xoff = true;
        risky.transfer.append_program_end = true;
        risky.transfer.program_start = "M82".into();
        risky.receive.max_receive_bytes = 0;
        let text = safety_warnings(&risky).join("\n");
        for expected in [
            "6 Datenbits",
            "RTS manuell",
            "Mehrere Flusssteuerungen",
            "Unbegrenzter Empfang",
            "Extrem kurze Timeouts",
            "Ergänzen von 'M30,M2'",
            "M82",
        ] {
            assert!(text.contains(expected), "{expected}");
        }
    }

    #[test]
    fn ids_and_repr() {
        assert!(valid_profile_id("dialog11_7n1"));
        assert!(!valid_profile_id("_x") && !valid_profile_id("") && !valid_profile_id("ABC"));
        assert_eq!(py_repr(&json!(4800.0)), "4800.0");
        assert_eq!(py_repr(&json!(true)), "True");
        assert_eq!(py_repr(&json!("x")), "'x'");
        assert_eq!(py_repr(&Value::Null), "None");
    }
}
