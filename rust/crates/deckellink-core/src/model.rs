//! Configuration and domain models.
//!
//! All machine-specific values for the DIALOG 11 are *assumptions* unless a
//! profile is explicitly marked `verified` (see `docs/VERIFICATION_CHECKLIST.md`).

use std::fmt;

/// Implements `as_str`, `parse` and `ALL` for a fieldless enum with string values.
macro_rules! string_enum {
    ($name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            pub fn as_str(self) -> &'static str {
                match self { $($name::$variant => $text),+ }
            }

            /// Case-insensitive parse of the JSON value.
            pub fn parse(text: &str) -> Option<Self> {
                let lower = text.to_ascii_lowercase();
                match lower.as_str() { $($text => Some($name::$variant),)+ _ => None }
            }

            pub fn allowed() -> String {
                Self::ALL.iter().map(|v| v.as_str()).collect::<Vec<_>>().join(", ")
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Parity {
    #[default]
    None,
    Even,
    Odd,
    Mark,
    Space,
}
string_enum!(Parity { None => "none", Even => "even", Odd => "odd", Mark => "mark", Space => "space" });

impl Parity {
    pub fn letter(self) -> char {
        match self {
            Parity::None => 'N',
            Parity::Even => 'E',
            Parity::Odd => 'O',
            Parity::Mark => 'M',
            Parity::Space => 'S',
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Parity::None => "Keine",
            Parity::Even => "Gerade (Even)",
            Parity::Odd => "Ungerade (Odd)",
            Parity::Mark => "Mark",
            Parity::Space => "Space",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineEnding {
    #[default]
    Crlf,
    Lf,
    Cr,
}
string_enum!(LineEnding { Crlf => "crlf", Lf => "lf", Cr => "cr" });

impl LineEnding {
    pub fn sequence(self) -> &'static [u8] {
        match self {
            LineEnding::Crlf => b"\r\n",
            LineEnding::Lf => b"\n",
            LineEnding::Cr => b"\r",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VerificationStatus {
    #[default]
    Unverified,
    Verified,
}
string_enum!(VerificationStatus { Unverified => "unverified", Verified => "verified" });

impl VerificationStatus {
    pub fn label(self) -> &'static str {
        match self {
            VerificationStatus::Verified => "an Maschine verifiziert",
            VerificationStatus::Unverified => "nicht verifiziert",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabHandling {
    Reject,
    #[default]
    Replace,
    Keep,
}
string_enum!(TabHandling { Reject => "reject", Replace => "replace", Keep => "keep" });

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WhitespaceHandling {
    None,
    #[default]
    Trailing,
    Both,
}
string_enum!(WhitespaceHandling { None => "none", Trailing => "trailing", Both => "both" });

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CommentHandling {
    #[default]
    Keep,
    Remove,
}
string_enum!(CommentHandling { Keep => "keep", Remove => "remove" });

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EndDetection {
    Inactivity,
    EndSequence,
    #[default]
    Both,
}
string_enum!(EndDetection { Inactivity => "inactivity", EndSequence => "end_sequence", Both => "both" });

impl EndDetection {
    pub fn uses_end_sequence(self) -> bool {
        matches!(self, EndDetection::EndSequence | EndDetection::Both)
    }

    pub fn uses_inactivity(self) -> bool {
        matches!(self, EndDetection::Inactivity | EndDetection::Both)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProtocolMode {
    #[default]
    Plain,
    Lsv2,
}
string_enum!(ProtocolMode { Plain => "plain", Lsv2 => "lsv2" });

impl ProtocolMode {
    pub fn label(self) -> &'static str {
        match self {
            ProtocolMode::Plain => "Ohne Protokoll (Klartext)",
            ProtocolMode::Lsv2 => "LSV2 (DIN 66019, Telegramme mit Quittung)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BccMode {
    #[default]
    TextEtx,
    Transmitted,
    None,
}
string_enum!(BccMode { TextEtx => "text_etx", Transmitted => "transmitted", None => "none" });

pub const STANDARD_BAUD_RATES: &[i64] =
    &[50, 75, 110, 134, 150, 200, 300, 600, 1200, 1800, 2400, 4800, 9600, 19200, 38400, 57600, 115200];
pub const DATA_BITS: &[i64] = &[5, 6, 7, 8];
/// Stop bits as half-bit units (1 = 2, 1.5 = 3, 2 = 4) to avoid float comparisons.
pub const STOP_BITS_HALVES: &[i64] = &[2, 3, 4];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineInfo {
    pub manufacturer: String,
    pub model: String,
    pub control: String,
}

impl Default for MachineInfo {
    fn default() -> Self {
        Self { manufacturer: "DECKEL".into(), model: "FP2A".into(), control: "DIALOG 11".into() }
    }
}

/// Stop bits: 1, 1.5 or 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StopBits {
    #[default]
    One,
    OnePointFive,
    Two,
}

impl StopBits {
    pub fn from_halves(halves: i64) -> Option<Self> {
        match halves {
            2 => Some(StopBits::One),
            3 => Some(StopBits::OnePointFive),
            4 => Some(StopBits::Two),
            _ => None,
        }
    }

    pub fn as_f64(self) -> f64 {
        match self {
            StopBits::One => 1.0,
            StopBits::OnePointFive => 1.5,
            StopBits::Two => 2.0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            StopBits::One => "1",
            StopBits::OnePointFive => "1.5",
            StopBits::Two => "2",
        }
    }
}

/// RS-232 parameters. Defaults are conservative assumptions, not verified values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerialSettings {
    pub baud_rate: u32,
    pub data_bits: u8,
    pub parity: Parity,
    pub stop_bits: StopBits,
    pub rts_cts: bool,
    pub xon_xoff: bool,
    pub dsr_dtr: bool,
    pub initial_rts: bool,
    pub initial_dtr: bool,
    pub require_cts_before_send: bool,
    pub require_dsr_before_send: bool,
    pub read_timeout_ms: u64,
    pub write_timeout_ms: u64,
    pub inter_byte_timeout_ms: u64,
    pub handshake_timeout_ms: u64,
}

impl Default for SerialSettings {
    fn default() -> Self {
        Self {
            baud_rate: 4800,
            data_bits: 7,
            parity: Parity::None,
            stop_bits: StopBits::One,
            rts_cts: false,
            xon_xoff: false,
            dsr_dtr: false,
            initial_rts: true,
            initial_dtr: false,
            require_cts_before_send: false,
            require_dsr_before_send: false,
            read_timeout_ms: 1000,
            write_timeout_ms: 10_000,
            inter_byte_timeout_ms: 0,
            handshake_timeout_ms: 30_000,
        }
    }
}

impl SerialSettings {
    pub fn frame(&self) -> String {
        format!("{}{}{}", self.data_bits, self.parity.letter(), self.stop_bits.label())
    }

    pub fn flow_control_label(&self) -> String {
        let mut parts = Vec::new();
        if self.rts_cts {
            parts.push("RTS/CTS");
        }
        if self.xon_xoff {
            parts.push("XON/XOFF");
        }
        if self.dsr_dtr {
            parts.push("DSR/DTR");
        }
        if parts.is_empty() {
            "Kein Handshake".to_string()
        } else {
            parts.join(" + ")
        }
    }

    pub fn bits_per_character(&self) -> f64 {
        let parity_bits = if self.parity == Parity::None { 0.0 } else { 1.0 };
        1.0 + f64::from(self.data_bits) + parity_bits + self.stop_bits.as_f64()
    }

    pub fn bytes_per_second(&self) -> f64 {
        f64::from(self.baud_rate) / self.bits_per_character()
    }

    pub fn summary(&self) -> String {
        format!("{} Baud | {} | {}", self.baud_rate, self.frame(), self.flow_control_label())
    }
}

/// Settings for preparing and sending NC data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferSettings {
    pub encoding: String,
    pub line_ending: LineEnding,
    pub program_start: String,
    pub program_end: String,
    pub append_program_end: bool,
    pub start_delay_ms: u64,
    pub end_delay_ms: u64,
    pub character_delay_ms: u64,
    pub packet_size: u64,
    pub packet_delay_ms: u64,
    pub remove_blank_lines: bool,
    pub strip_null_bytes: bool,
    pub skip_semicolon_lines: bool,
    pub normalize_line_endings: bool,
    pub tab_handling: TabHandling,
    pub whitespace: WhitespaceHandling,
    pub comment_handling: CommentHandling,
    pub comment_open: String,
    pub comment_close: String,
    pub ensure_final_newline: bool,
    pub send_eot_trailer: bool,
    pub max_line_length: u64,
    pub max_program_bytes: u64,
    pub close_port_after_send: bool,
}

impl Default for TransferSettings {
    fn default() -> Self {
        Self {
            encoding: "ascii".into(),
            line_ending: LineEnding::Crlf,
            program_start: String::new(),
            program_end: "M30".into(),
            append_program_end: false,
            start_delay_ms: 1000,
            end_delay_ms: 1000,
            character_delay_ms: 0,
            packet_size: 0,
            packet_delay_ms: 0,
            remove_blank_lines: true,
            strip_null_bytes: true,
            skip_semicolon_lines: false,
            normalize_line_endings: true,
            tab_handling: TabHandling::Replace,
            whitespace: WhitespaceHandling::Trailing,
            comment_handling: CommentHandling::Keep,
            comment_open: "(".into(),
            comment_close: ")".into(),
            ensure_final_newline: true,
            send_eot_trailer: true,
            max_line_length: 120,
            max_program_bytes: 500_000,
            close_port_after_send: true,
        }
    }
}

/// Settings for receiving programs from the CNC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiveSettings {
    pub end_detection: EndDetection,
    pub end_sequence: String,
    pub inactivity_timeout_ms: u64,
    pub first_byte_timeout_ms: u64,
    pub end_grace_ms: u64,
    pub max_receive_bytes: u64,
    pub strip_null_bytes: bool,
    pub normalize_line_endings: bool,
    pub save_raw: bool,
    pub end_on_eot: bool,
    pub program_name_pattern: String,
}

impl Default for ReceiveSettings {
    fn default() -> Self {
        Self {
            end_detection: EndDetection::Both,
            end_sequence: "M30".into(),
            inactivity_timeout_ms: 5000,
            first_byte_timeout_ms: 120_000,
            end_grace_ms: 500,
            max_receive_bytes: 2_000_000,
            strip_null_bytes: true,
            normalize_line_endings: true,
            save_raw: true,
            end_on_eot: true,
            program_name_pattern: String::new(),
        }
    }
}

/// LSV2 / DIN 66019 link settings (every DIALOG 11 detail is an assumption).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolSettings {
    pub mode: ProtocolMode,
    pub transparent: bool,
    pub bcc_mode: BccMode,
    pub enq_interval_ms: u64,
    pub link_timeout_ms: u64,
    pub ack_timeout_ms: u64,
    pub drip_feed_ack_timeout_ms: u64,
    pub max_retries: u64,
    pub wack_poll_ms: u64,
    pub lines_per_telegram: u64,
    pub max_telegram_length: u64,
    pub include_line_ending: bool,
    pub start_telegrams: Vec<String>,
    pub end_telegrams: Vec<String>,
    pub send_eot_on_cancel: bool,
}

impl Default for ProtocolSettings {
    fn default() -> Self {
        Self {
            mode: ProtocolMode::Plain,
            transparent: true,
            bcc_mode: BccMode::TextEtx,
            enq_interval_ms: 2000,
            link_timeout_ms: 120_000,
            ack_timeout_ms: 5000,
            drip_feed_ack_timeout_ms: 600_000,
            max_retries: 3,
            wack_poll_ms: 1000,
            lines_per_telegram: 1,
            max_telegram_length: 256,
            include_line_ending: true,
            start_telegrams: Vec::new(),
            end_telegrams: Vec::new(),
            send_eot_on_cancel: true,
        }
    }
}

impl ProtocolSettings {
    pub fn is_lsv2(&self) -> bool {
        self.mode == ProtocolMode::Lsv2
    }
}

/// Transfer of machine parameter files (format of the DIALOG 11 not verified).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterSettings {
    pub allow_write: bool,
    pub inactivity_timeout_ms: u64,
    pub first_byte_timeout_ms: u64,
    pub max_bytes: u64,
    pub identification_pattern: String,
    pub max_backup_age_min: u64,
}

impl Default for ParameterSettings {
    fn default() -> Self {
        Self {
            allow_write: false,
            inactivity_timeout_ms: 5000,
            first_byte_timeout_ms: 120_000,
            max_bytes: 1_000_000,
            identification_pattern: String::new(),
            max_backup_age_min: 60,
        }
    }
}

/// A complete communication profile for one machine/control configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub id: String,
    pub display_name: String,
    pub machine: MachineInfo,
    pub verification_status: VerificationStatus,
    pub serial: SerialSettings,
    pub transfer: TransferSettings,
    pub receive: ReceiveSettings,
    pub protocol: ProtocolSettings,
    pub parameters: ParameterSettings,
    pub description: String,
    pub schema_version: u32,
    /// Built-in profiles are never overwritten (not part of equality semantics in Python; kept here).
    pub builtin: bool,
}

impl Profile {
    /// Internal fallback if no profile file could be loaded (values of profile A, unverified).
    pub fn fallback() -> Self {
        Self {
            id: "fallback_7n1_rtscts".into(),
            display_name: "Interner Notfall-Standard - 4800 7N1 RTS/CTS".into(),
            machine: MachineInfo::default(),
            verification_status: VerificationStatus::Unverified,
            serial: SerialSettings {
                rts_cts: true,
                initial_rts: true,
                require_cts_before_send: true,
                ..SerialSettings::default()
            },
            transfer: TransferSettings::default(),
            receive: ReceiveSettings::default(),
            protocol: ProtocolSettings::default(),
            parameters: ParameterSettings::default(),
            description: "Fest eingebautes Ersatzprofil, falls keine Profildatei geladen werden konnte. \
                          Werte entsprechen Profil A und sind nicht verifiziert."
                .into(),
            schema_version: 1,
            builtin: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_properties() {
        let s = Profile::fallback().serial;
        assert_eq!(s.summary(), "4800 Baud | 7N1 | RTS/CTS");
        assert!((s.bits_per_character() - 9.0).abs() < 1e-9);
        let even = SerialSettings { parity: Parity::Even, stop_bits: StopBits::OnePointFive, ..s.clone() };
        assert_eq!(even.frame(), "7E1.5");
        assert!((even.bits_per_character() - 10.5).abs() < 1e-9);
        let none = SerialSettings { rts_cts: false, ..s };
        assert_eq!(none.flow_control_label(), "Kein Handshake");
    }

    #[test]
    fn enums_parse_case_insensitive() {
        assert_eq!(Parity::parse("EVEN"), Some(Parity::Even));
        assert_eq!(Parity::parse("x"), None);
        assert_eq!(BccMode::parse("text_etx"), Some(BccMode::TextEtx));
        assert_eq!(EndDetection::allowed(), "inactivity, end_sequence, both");
        assert_eq!(LineEnding::Crlf.sequence(), b"\r\n");
        assert!(EndDetection::Both.uses_end_sequence() && EndDetection::Both.uses_inactivity());
        assert_eq!(StopBits::from_halves(3), Some(StopBits::OnePointFive));
        assert_eq!(StopBits::from_halves(5), None);
    }
}
