//! Low-level byte helpers shared by analyzer, transformer and receiver.

pub const CR: u8 = 0x0D;
pub const LF: u8 = 0x0A;
pub const TAB: u8 = 0x09;
pub const NUL: u8 = 0x00;
pub const DEL: u8 = 0x7F;
pub const EOT: u8 = 0x04;

/// One line with its exact line ending (empty for a last line without ending).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawLine {
    pub number: usize,
    pub content: Vec<u8>,
    pub ending: Vec<u8>,
}

/// Split into lines, keeping CRLF / CR / LF exactly; a trailing fragment becomes a last line.
pub fn split_lines(data: &[u8]) -> Vec<RawLine> {
    let mut lines = Vec::new();
    let mut start = 0usize;
    let mut pos = 0usize;
    while pos < data.len() {
        match data.get(pos) {
            Some(&CR) => {
                let crlf = data.get(pos + 1) == Some(&LF);
                let end = if crlf { pos + 2 } else { pos + 1 };
                lines.push(RawLine {
                    number: lines.len() + 1,
                    content: data.get(start..pos).unwrap_or_default().to_vec(),
                    ending: data.get(pos..end).unwrap_or_default().to_vec(),
                });
                pos = end;
                start = end;
            }
            Some(&LF) => {
                lines.push(RawLine {
                    number: lines.len() + 1,
                    content: data.get(start..pos).unwrap_or_default().to_vec(),
                    ending: vec![LF],
                });
                pos += 1;
                start = pos;
            }
            _ => pos += 1,
        }
    }
    if start < data.len() {
        lines.push(RawLine {
            number: lines.len() + 1,
            content: data.get(start..).unwrap_or_default().to_vec(),
            ending: Vec::new(),
        });
    }
    lines
}

pub fn ending_name(ending: &[u8]) -> &'static str {
    match ending {
        b"\r\n" => "crlf",
        b"\n" => "lf",
        b"\r" => "cr",
        _ => "none",
    }
}

/// Counts of (crlf, lf, cr).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EndingCounts {
    pub crlf: usize,
    pub lf: usize,
    pub cr: usize,
}

impl EndingCounts {
    pub fn get(&self, name: &str) -> usize {
        match name {
            "crlf" => self.crlf,
            "lf" => self.lf,
            "cr" => self.cr,
            _ => 0,
        }
    }

    pub fn used(&self) -> Vec<&'static str> {
        ["crlf", "lf", "cr"].into_iter().filter(|n| self.get(n) > 0).collect()
    }
}

pub fn count_line_endings(lines: &[RawLine]) -> EndingCounts {
    let mut counts = EndingCounts::default();
    for line in lines {
        match ending_name(&line.ending) {
            "crlf" => counts.crlf += 1,
            "lf" => counts.lf += 1,
            "cr" => counts.cr += 1,
            _ => {}
        }
    }
    counts
}

/// Split a trailing EOT (optionally followed by CR/LF/NUL) from `data`.
///
/// The DIALOG 11 ends its V.24 output with EOT + CRLF (observed in real machine
/// data files). Only an EOT at the very end is treated as transmission end.
pub fn split_eot_trailer(data: &[u8]) -> (&[u8], &[u8]) {
    let tail_len = data.iter().rev().take_while(|b| matches!(**b, CR | LF | NUL)).count();
    let before_tail = data.len().saturating_sub(tail_len);
    if before_tail > 0 && data.get(before_tail - 1) == Some(&EOT) {
        let split = before_tail - 1;
        (data.get(..split).unwrap_or_default(), data.get(split..).unwrap_or_default())
    } else {
        (data, &[])
    }
}

/// `"M30,M2"` -> `["M30", "M2"]` (comma or semicolon separated).
pub fn program_end_alternatives(token: &str) -> Vec<String> {
    token.split([',', ';']).map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect()
}

/// The first alternative - used when a program end has to be appended.
pub fn primary_program_end(token: &str) -> String {
    program_end_alternatives(token).into_iter().next().unwrap_or_default()
}

/// Matcher for stand-alone program end words (`M30` matches `N100M30`, `m30`, `M030`; not `M300`, `XM30`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramEndMatcher {
    words: Vec<Word>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Word {
    /// Letter + number: leading zeros allowed (`M2` matches `M02`).
    Nc { letter: u8, number: Vec<u8> },
    /// Any other literal text (compared case-insensitively).
    Literal(Vec<u8>),
}

impl ProgramEndMatcher {
    pub fn new(token: &str) -> Option<Self> {
        let words: Vec<Word> = program_end_alternatives(token)
            .iter()
            .map(|alt| {
                let bytes = alt.as_bytes();
                match bytes.split_first() {
                    Some((&letter, rest))
                        if letter.is_ascii_alphabetic() && !rest.is_empty() && rest.iter().all(u8::is_ascii_digit) =>
                    {
                        let trimmed: Vec<u8> = rest.iter().copied().skip_while(|b| *b == b'0').collect();
                        Word::Nc { letter: letter.to_ascii_uppercase(), number: trimmed }
                    }
                    _ => Word::Literal(bytes.to_ascii_uppercase()),
                }
            })
            .collect();
        if words.is_empty() {
            None
        } else {
            Some(Self { words })
        }
    }

    /// True if any alternative occurs as a stand-alone word in `content`.
    pub fn is_match(&self, content: &[u8]) -> bool {
        self.find(content).is_some()
    }

    /// First match as `(start, end)`; at equal start the first alternative wins (like a regex alternation).
    pub fn find(&self, content: &[u8]) -> Option<(usize, usize)> {
        (0..content.len())
            .find_map(|start| self.words.iter().find_map(|w| match_end(w, content, start).map(|end| (start, end))))
    }
}

fn match_end(word: &Word, content: &[u8], start: usize) -> Option<usize> {
    if start > 0 {
        if let Some(prev) = content.get(start - 1) {
            if prev.is_ascii_alphabetic() || *prev == b'.' {
                return None;
            }
        }
    }
    let end = match word {
        Word::Nc { letter, number } => {
            if content.get(start).map(u8::to_ascii_uppercase) != Some(*letter) {
                return None;
            }
            let mut pos = start + 1;
            if number.is_empty() {
                // the word was e.g. "M0": accept one or more zeros
                let zeros = content.get(pos..).unwrap_or_default().iter().take_while(|b| **b == b'0').count();
                if zeros == 0 {
                    return None;
                }
                pos + zeros
            } else {
                while content.get(pos) == Some(&b'0') {
                    pos += 1;
                }
                if content.get(pos..pos + number.len()) != Some(number.as_slice()) {
                    return None;
                }
                pos += number.len();
                pos
            }
        }
        Word::Literal(text) => {
            let slice = content.get(start..start + text.len())?;
            if slice.to_ascii_uppercase() != *text {
                return None;
            }
            start + text.len()
        }
    };
    if content.get(end).is_some_and(u8::is_ascii_digit) {
        None
    } else {
        Some(end)
    }
}

/// Remove comment spans such as `(TEXT)`. Unterminated comments run to line end.
pub fn remove_comments(content: &[u8], open: &str, close: &str) -> Vec<u8> {
    let open_b = open.as_bytes();
    let close_b = close.as_bytes();
    if open_b.is_empty() {
        return content.to_vec();
    }
    let mut result = Vec::with_capacity(content.len());
    let mut i = 0usize;
    while i < content.len() {
        let rest = content.get(i..).unwrap_or_default();
        let Some(rel_start) = find(rest, open_b) else {
            result.extend_from_slice(rest);
            break;
        };
        result.extend_from_slice(rest.get(..rel_start).unwrap_or_default());
        if close_b.is_empty() {
            break;
        }
        let after_open = i + rel_start + open_b.len();
        let Some(rel_end) = find(content.get(after_open..).unwrap_or_default(), close_b) else {
            break;
        };
        i = after_open + rel_end + close_b.len();
    }
    result
}

/// First position of `needle` in `haystack`.
pub fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

pub fn strip_bytes<'a>(data: &'a [u8], chars: &[u8]) -> &'a [u8] {
    let start = data.iter().position(|b| !chars.contains(b)).unwrap_or(data.len());
    let end = data.iter().rposition(|b| !chars.contains(b)).map_or(start, |p| p + 1);
    data.get(start..end).unwrap_or_default()
}

pub fn rstrip_bytes<'a>(data: &'a [u8], chars: &[u8]) -> &'a [u8] {
    let end = data.iter().rposition(|b| !chars.contains(b)).map_or(0, |p| p + 1);
    data.get(..end).unwrap_or_default()
}

pub fn lstrip_bytes<'a>(data: &'a [u8], chars: &[u8]) -> &'a [u8] {
    let start = data.iter().position(|b| !chars.contains(b)).unwrap_or(data.len());
    data.get(start..).unwrap_or_default()
}

pub fn is_semicolon_line(content: &[u8]) -> bool {
    lstrip_bytes(content, b" \t").first() == Some(&b';')
}

pub fn is_blank(content: &[u8]) -> bool {
    strip_bytes(content, b" \t\x00").is_empty()
}

/// Highest byte value that fits in a character of `data_bits` (capped at ASCII).
pub fn max_value_for_data_bits(data_bits: u8) -> u8 {
    let shifted = 1u16.checked_shl(u32::from(data_bits)).unwrap_or(256).saturating_sub(1);
    u8::try_from(shifted.min(0x7F)).unwrap_or(0x7F)
}

/// Control characters other than CR, LF, TAB and NUL (handled separately).
pub fn is_disallowed_control(byte: u8) -> bool {
    (byte < 0x20 && !matches!(byte, CR | LF | TAB | NUL)) || byte == DEL
}

/// Printable ASCII representation with visible control characters.
pub fn format_ascii(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len());
    for &b in data {
        match b {
            CR => out.push_str("<CR>"),
            LF => out.push_str("<LF>"),
            TAB => out.push_str("<TAB>"),
            NUL => out.push_str("<NUL>"),
            0x02 => out.push_str("<STX>"),
            0x03 => out.push_str("<ETX>"),
            EOT => out.push_str("<EOT>"),
            0x05 => out.push_str("<ENQ>"),
            0x10 => out.push_str("<DLE>"),
            0x15 => out.push_str("<NAK>"),
            0x20..=0x7E => out.push(char::from(b)),
            _ => out.push_str(&format!("<{b:02X}>")),
        }
    }
    out
}

pub fn format_hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_lines_keeps_endings() {
        let lines = split_lines(b"A\r\nB\nC\rD");
        let parts: Vec<_> = lines.iter().map(|l| (l.content.clone(), l.ending.clone())).collect();
        assert_eq!(
            parts,
            vec![
                (b"A".to_vec(), b"\r\n".to_vec()),
                (b"B".to_vec(), b"\n".to_vec()),
                (b"C".to_vec(), b"\r".to_vec()),
                (b"D".to_vec(), Vec::new()),
            ]
        );
        assert_eq!(lines[3].number, 4);
        assert!(split_lines(b"").is_empty());
        assert_eq!(split_lines(b"\r\n\r\n").len(), 2);
        let counts = count_line_endings(&lines);
        assert_eq!((counts.crlf, counts.lf, counts.cr), (1, 1, 1));
        assert_eq!(counts.used(), vec!["crlf", "lf", "cr"]);
    }

    #[test]
    fn eot_trailer() {
        assert_eq!(split_eot_trailer(b"A\r\n\x04\r\n"), (&b"A\r\n"[..], &b"\x04\r\n"[..]));
        assert_eq!(split_eot_trailer(b"A\r\n\x04"), (&b"A\r\n"[..], &b"\x04"[..]));
        assert_eq!(split_eot_trailer(b"A\x04B\r\n"), (&b"A\x04B\r\n"[..], &b""[..]));
        assert_eq!(split_eot_trailer(b"\x04"), (&b""[..], &b"\x04"[..]));
        assert_eq!(split_eot_trailer(b"\r\n"), (&b"\r\n"[..], &b""[..]));
    }

    #[test]
    fn program_end_matching() {
        assert_eq!(program_end_alternatives("M30, M2"), vec!["M30", "M2"]);
        assert_eq!(program_end_alternatives("M30;M2"), vec!["M30", "M2"]);
        assert_eq!(primary_program_end("M30,M2"), "M30");
        assert!(ProgramEndMatcher::new("").is_none());
        let m = ProgramEndMatcher::new("M30,M2").unwrap();
        for text in [&b"N10 M30"[..], b"N10 M2", b"N10 M02", b"M2", b"N10 m2", b"N100M30"] {
            assert!(m.is_match(text), "{}", String::from_utf8_lossy(text));
        }
        for text in [&b"N10 M20"[..], b"N10 M230", b"N10 M3", b"N10 X1.M2", b"N10 AM2", b"M300"] {
            assert!(!m.is_match(text), "{}", String::from_utf8_lossy(text));
        }
        let zero = ProgramEndMatcher::new("M0").unwrap();
        assert!(zero.is_match(b"M00") && !zero.is_match(b"M") && !zero.is_match(b"M01"));
        let literal = ProgramEndMatcher::new("%").unwrap();
        assert!(literal.is_match(b"%") && !literal.is_match(b"x"));
    }

    #[test]
    fn comments_and_classification() {
        assert_eq!(remove_comments(b"N10 (A) G0 (B", "(", ")"), b"N10  G0 ");
        assert_eq!(remove_comments(b"N10 ;x", ";", ""), b"N10 ");
        assert_eq!(remove_comments(b"N10", "", ")"), b"N10");
        assert!(is_semicolon_line(b"  ;x") && !is_semicolon_line(b"x;"));
        assert!(is_blank(b" \t\x00") && !is_blank(b" a"));
        assert_eq!(max_value_for_data_bits(7), 0x7F);
        assert_eq!(max_value_for_data_bits(6), 0x3F);
        assert_eq!(max_value_for_data_bits(8), 0x7F);
        assert!(is_disallowed_control(0x1B) && is_disallowed_control(DEL) && !is_disallowed_control(TAB));
        assert_eq!(format_ascii(b"A\r\n\x04\x80"), "A<CR><LF><EOT><80>");
        assert_eq!(format_hex(b"\x01\xff"), "01 FF");
        assert_eq!(strip_bytes(b"  x  ", b" "), b"x");
        assert_eq!(rstrip_bytes(b"  x  ", b" "), b"  x");
        assert_eq!(find(b"abc", b"c"), Some(2));
    }
}
