//! Read-only analysis of NC data. Never modifies its input.

use std::cmp::Reverse;

use crate::model::{SerialSettings, TabHandling, TransferSettings};
use crate::nc::checks::{
    count_line_endings, is_blank, is_disallowed_control, is_semicolon_line, max_value_for_data_bits, remove_comments,
    split_eot_trailer, split_lines, strip_bytes, EndingCounts, ProgramEndMatcher, RawLine, NUL, TAB,
};
use crate::nc::{AnalysisResult, Finding, Severity};
use crate::storage::sha256_hex;

pub const MAX_REPORTED_LINES: usize = 200;

#[derive(Default)]
struct Collector {
    findings: Vec<Finding>,
}

impl Collector {
    fn add(&mut self, code: &str, severity: Severity, message: String, lines: &[usize], count: usize) {
        let shown: Vec<usize> = lines.iter().take(MAX_REPORTED_LINES).copied().collect();
        let total = if count == 0 { lines.len() } else { count };
        self.findings.push(Finding { code: code.into(), severity, message, lines: shown, count: total });
    }
}

/// `1234567` -> `1.234.567` (German thousands separator).
pub fn thousands(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('.');
        }
        out.push(c);
    }
    out
}

/// Analyse `data` according to `settings`; `serial` enables the data-bit range check.
pub fn analyze(data: &[u8], settings: &TransferSettings, serial: Option<&SerialSettings>) -> AnalysisResult {
    let mut col = Collector::default();
    let full = data;
    let (body, trailer) = split_eot_trailer(full);
    let lines = split_lines(body);
    let endings = count_line_endings(&lines);
    let longest = lines.iter().map(|l| l.content.len()).max().unwrap_or(0);
    let matcher = ProgramEndMatcher::new(&settings.program_end);

    if body.is_empty() {
        col.add("EMPTY_FILE", Severity::Error, "Die Datei ist leer.".into(), &[], 0);
        return AnalysisResult {
            findings: col.findings,
            byte_count: full.len(),
            line_count: 0,
            line_endings: endings,
            longest_line: 0,
            has_program_end: false,
            sha256: sha256_hex(full),
        };
    }
    if !trailer.is_empty() {
        let suffix =
            if settings.send_eot_trailer { " Es wird mitgesendet." } else { " Es wird laut Profil nicht gesendet." };
        col.add(
            "TRANSMISSION_END",
            Severity::Info,
            format!(
                "Die Datei endet mit dem Übertragungsende-Zeichen EOT (0x04), wie es die DIALOG 11 ausgibt.{suffix}"
            ),
            &[lines.len() + 1],
            0,
        );
    }
    let max_bytes = usize::try_from(settings.max_program_bytes).unwrap_or(usize::MAX);
    if max_bytes > 0 && body.len() > max_bytes {
        col.add(
            "FILE_TOO_LARGE",
            Severity::Warning,
            format!(
                "Sehr große Datei ({} Bytes, Grenze laut Profil {} Bytes). Die Speicherkapazität der Steuerung ist \
                 nicht verifiziert.",
                thousands(body.len()),
                thousands(max_bytes)
            ),
            &[],
            0,
        );
    }
    check_bytes(&lines, settings, serial, &mut col);
    check_line_endings(&lines, endings, settings, &mut col);
    check_lines(&lines, settings, &mut col);
    let has_end = check_program_end(&lines, settings, matcher.as_ref(), &mut col);
    if !settings.program_start.trim().is_empty() {
        col.add(
            "UNVERIFIED_START_SEQUENCE",
            Severity::Warning,
            format!(
                "Profil fügt die Startsequenz '{}' ein. Diese Sequenz ist für die DIALOG 11 nicht verifiziert.",
                settings.program_start.trim()
            ),
            &[],
            0,
        );
    }
    let mut findings = col.findings;
    findings.sort_by_key(|f| Reverse(f.severity.rank()));
    AnalysisResult {
        findings,
        byte_count: full.len(),
        line_count: lines.len(),
        line_endings: endings,
        longest_line: longest,
        has_program_end: has_end,
        sha256: sha256_hex(full),
    }
}

fn check_bytes(lines: &[RawLine], settings: &TransferSettings, serial: Option<&SerialSettings>, col: &mut Collector) {
    let bits = serial.map_or(7, |s| s.data_bits);
    let max_value = max_value_for_data_bits(bits);
    let (mut null_lines, mut high_lines, mut ctrl_lines, mut tab_lines) = (vec![], vec![], vec![], vec![]);
    let (mut null_count, mut high_count, mut ctrl_count) = (0usize, 0usize, 0usize);
    for line in lines {
        let (mut has_null, mut has_high, mut has_ctrl) = (false, false, false);
        for &b in &line.content {
            if b == NUL {
                null_count += 1;
                has_null = true;
            } else if b > max_value {
                high_count += 1;
                has_high = true;
            } else if is_disallowed_control(b) {
                ctrl_count += 1;
                has_ctrl = true;
            }
        }
        if has_null {
            null_lines.push(line.number);
        }
        if has_high {
            high_lines.push(line.number);
        }
        if has_ctrl {
            ctrl_lines.push(line.number);
        }
        if line.content.contains(&TAB) {
            tab_lines.push(line.number);
        }
    }
    if null_count > 0 {
        if settings.strip_null_bytes {
            col.add(
                "NULL_BYTES",
                Severity::Warning,
                format!("{null_count} Nullbyte(s) gefunden. Sie werden laut Profil vor dem Senden entfernt."),
                &null_lines,
                null_count,
            );
        } else {
            col.add(
                "NULL_BYTES",
                Severity::Error,
                format!("{null_count} Nullbyte(s) gefunden und Entfernen ist im Profil deaktiviert."),
                &null_lines,
                null_count,
            );
        }
    }
    if high_count > 0 {
        col.add(
            "NON_ASCII",
            Severity::Error,
            format!(
                "{high_count} Zeichen sind mit {bits} Datenbits bzw. 7-Bit-ASCII nicht darstellbar (Bytewert über \
                 0x{max_value:02X}), z. B. Umlaute."
            ),
            &high_lines,
            high_count,
        );
    }
    if ctrl_count > 0 {
        col.add(
            "CONTROL_CHARS",
            Severity::Error,
            format!("{ctrl_count} unzulässige Steuerzeichen gefunden (z. B. ESC, DEL, XON/XOFF)."),
            &ctrl_lines,
            ctrl_count,
        );
    }
    if !tab_lines.is_empty() {
        let (severity, message) = match settings.tab_handling {
            TabHandling::Reject => (Severity::Error, "Tabulatoren gefunden. Laut Profil werden Tabs abgelehnt."),
            TabHandling::Replace => (Severity::Info, "Tabulatoren gefunden. Sie werden durch Leerzeichen ersetzt."),
            TabHandling::Keep => (
                Severity::Warning,
                "Tabulatoren werden unverändert gesendet. Ob die Steuerung Tabs akzeptiert, ist nicht verifiziert.",
            ),
        };
        col.add("TABS", severity, message.into(), &tab_lines, 0);
    }
}

fn check_line_endings(lines: &[RawLine], endings: EndingCounts, settings: &TransferSettings, col: &mut Collector) {
    let used = endings.used();
    let target = settings.line_ending.as_str();
    if used.len() > 1 {
        let detail = used
            .iter()
            .map(|name| format!("{}: {}", name.to_uppercase(), endings.get(name)))
            .collect::<Vec<_>>()
            .join(", ");
        if settings.normalize_line_endings {
            col.add(
                "MIXED_LINE_ENDINGS",
                Severity::Info,
                format!("Gemischte Zeilenenden ({detail}). Sie werden auf {} vereinheitlicht.", target.to_uppercase()),
                &[],
                0,
            );
        } else {
            col.add(
                "MIXED_LINE_ENDINGS",
                Severity::Warning,
                format!("Gemischte Zeilenenden ({detail}) und Normalisierung ist deaktiviert."),
                &[],
                0,
            );
        }
    } else if let Some(only) = used.first() {
        if *only != target {
            let severity = if settings.normalize_line_endings { Severity::Info } else { Severity::Warning };
            let suffix = if settings.normalize_line_endings { " Wird umgewandelt." } else { "" };
            col.add(
                "FOREIGN_LINE_ENDINGS",
                severity,
                format!("Datei verwendet {}, Profil erwartet {}.{suffix}", only.to_uppercase(), target.to_uppercase()),
                &[],
                0,
            );
        }
    }
    if let Some(last) = lines.last() {
        if last.ending.is_empty() {
            let severity = if settings.ensure_final_newline { Severity::Info } else { Severity::Warning };
            let suffix = if settings.ensure_final_newline { " Er wird ergänzt." } else { "" };
            col.add(
                "MISSING_FINAL_NEWLINE",
                severity,
                format!("Die letzte Zeile hat keinen abschließenden Zeilenumbruch.{suffix}"),
                &[last.number],
                0,
            );
        }
    }
}

fn check_lines(lines: &[RawLine], settings: &TransferSettings, col: &mut Collector) {
    let (mut blank, mut semicolon, mut long_lines, mut whitespace) = (vec![], vec![], vec![], vec![]);
    let max_len = usize::try_from(settings.max_line_length).unwrap_or(usize::MAX);
    for line in lines {
        if is_blank(&line.content) {
            if !(line.ending.is_empty() && line.content.is_empty()) {
                blank.push(line.number);
            }
            continue;
        }
        if is_semicolon_line(&line.content) {
            semicolon.push(line.number);
        }
        if max_len > 0 && line.content.len() > max_len {
            long_lines.push(line.number);
        }
        if strip_bytes(&line.content, b" \t") != line.content.as_slice() {
            whitespace.push(line.number);
        }
    }
    if !blank.is_empty() {
        let suffix = if settings.remove_blank_lines { " Sie werden entfernt." } else { "" };
        col.add("BLANK_LINES", Severity::Info, format!("{} Leerzeile(n).{suffix}", blank.len()), &blank, 0);
    }
    if !semicolon.is_empty() {
        let (severity, suffix) = if settings.skip_semicolon_lines {
            (Severity::Info, " Sie werden laut Profil entfernt.")
        } else {
            (Severity::Warning, " Ob die Steuerung diese Zeilen akzeptiert, ist nicht verifiziert.")
        };
        col.add(
            "SEMICOLON_LINES",
            severity,
            format!("{} Zeile(n) beginnen mit Semikolon.{suffix}", semicolon.len()),
            &semicolon,
            0,
        );
    }
    if !long_lines.is_empty() {
        col.add(
            "LONG_LINES",
            Severity::Warning,
            format!(
                "{} Zeile(n) länger als {} Zeichen (maximale Zeilenlänge der Steuerung nicht verifiziert).",
                long_lines.len(),
                settings.max_line_length
            ),
            &long_lines,
            0,
        );
    }
    if !whitespace.is_empty() {
        col.add(
            "SURROUNDING_WHITESPACE",
            Severity::Info,
            format!(
                "{} Zeile(n) mit führenden oder nachgestellten Leerzeichen (Behandlung laut Profil: {}).",
                whitespace.len(),
                settings.whitespace.as_str()
            ),
            &whitespace,
            0,
        );
    }
}

fn check_program_end(
    lines: &[RawLine],
    settings: &TransferSettings,
    matcher: Option<&ProgramEndMatcher>,
    col: &mut Collector,
) -> bool {
    let Some(matcher) = matcher else { return false };
    let mut end_line: Option<usize> = None;
    let mut after = Vec::new();
    for line in lines {
        let content = remove_comments(&line.content, &settings.comment_open, &settings.comment_close);
        if end_line.is_none() {
            if matcher.is_match(&content) {
                end_line = Some(line.number);
            }
        } else if !is_blank(&content) && !strip_bytes(&content, b" \t%").is_empty() {
            after.push(line.number);
        }
    }
    let Some(end_line) = end_line else {
        let message = if settings.append_program_end {
            format!(
                "Programmende '{}' fehlt. Das Profil erlaubt das Ergänzen - dies erfolgt nur nach ausdrücklicher \
                 Bestätigung.",
                settings.program_end
            )
        } else {
            format!("Programmende '{}' wurde nicht gefunden.", settings.program_end)
        };
        col.add("MISSING_PROGRAM_END", Severity::Warning, message, &[], 0);
        return false;
    };
    if !after.is_empty() {
        col.add(
            "DATA_AFTER_PROGRAM_END",
            Severity::Warning,
            format!("Nach '{}' (Zeile {end_line}) folgen weitere Daten.", settings.program_end),
            &after,
            0,
        );
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LineEnding, WhitespaceHandling};

    fn settings() -> TransferSettings {
        TransferSettings { program_end: "M30,M2".into(), ..TransferSettings::default() }
    }

    #[test]
    fn clean_program() {
        let r = analyze(b"N10 G00 X0\r\nN20 M30\r\n", &settings(), None);
        assert!(!r.has_errors() && !r.has_warnings() && r.has_program_end);
        assert_eq!((r.line_count, r.byte_count), (2, 21));
    }

    #[test]
    fn findings() {
        let data = b"N10 G00\tX0  \r\n\r\nN20 X\x00\r\n;K\nN30 \xc4\x1b\r\nN40 M30\r\nN50 G0\r\nOHNE_ENDE";
        let r = analyze(data, &settings(), None);
        for code in [
            "TABS",
            "NULL_BYTES",
            "NON_ASCII",
            "CONTROL_CHARS",
            "MIXED_LINE_ENDINGS",
            "MISSING_FINAL_NEWLINE",
            "BLANK_LINES",
            "SEMICOLON_LINES",
            "SURROUNDING_WHITESPACE",
            "DATA_AFTER_PROGRAM_END",
        ] {
            assert!(r.has(code), "{code}");
        }
        assert!(r.has_errors());
        assert_eq!(r.findings.first().map(|f| f.severity), Some(Severity::Error));
        assert_eq!(r.get("DATA_AFTER_PROGRAM_END").map(|f| f.lines.clone()), Some(vec![7, 8]));
    }

    #[test]
    fn eot_and_empty() {
        let r = analyze(b"N10 G00\r\nN20 M30\r\n\x04\r\n", &settings(), None);
        assert!(!r.has_errors() && r.has("TRANSMISSION_END") && r.byte_count == 21);
        assert!(analyze(b"\x04\r\n", &settings(), None).has("EMPTY_FILE"));
        assert!(analyze(b"", &settings(), None).has("EMPTY_FILE"));
        let bad = analyze(b"N10 G00\r\n\x04N20 M30\r\n", &settings(), None);
        assert!(bad.has("CONTROL_CHARS"));
    }

    #[test]
    fn profile_dependent_severities() {
        let reject = TransferSettings { tab_handling: TabHandling::Reject, ..settings() };
        assert!(analyze(b"A\tB\r\nM30\r\n", &reject, None).has_errors());
        let keep = TransferSettings {
            tab_handling: TabHandling::Keep,
            strip_null_bytes: false,
            normalize_line_endings: false,
            whitespace: WhitespaceHandling::Both,
            line_ending: LineEnding::Lf,
            append_program_end: true,
            program_start: "%".into(),
            max_program_bytes: 4,
            max_line_length: 3,
            ..settings()
        };
        let r = analyze(b"A\tB\x00\r\n", &keep, None);
        for code in [
            "TABS",
            "NULL_BYTES",
            "FOREIGN_LINE_ENDINGS",
            "MISSING_PROGRAM_END",
            "UNVERIFIED_START_SEQUENCE",
            "FILE_TOO_LARGE",
            "LONG_LINES",
        ] {
            assert!(r.has(code), "{code}");
        }
        assert_eq!(r.get("NULL_BYTES").map(|f| f.severity), Some(Severity::Error));
        let six_bits = SerialSettings { data_bits: 6, ..SerialSettings::default() };
        assert!(analyze(b"a\r\nM30\r\n", &settings(), Some(&six_bits)).has("NON_ASCII"));
        let no_end = TransferSettings { program_end: String::new(), ..settings() };
        assert!(!analyze(b"A\r\n", &no_end, None).has("MISSING_PROGRAM_END"));
    }

    #[test]
    fn thousands_format() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(1234), "1.234");
        assert_eq!(thousands(1_234_567), "1.234.567");
    }
}
