//! Transformation of NC data into transfer bytes. Works on a copy; the original stays untouched.

use crate::error::TransformError;
use crate::model::{CommentHandling, TabHandling, TransferSettings, WhitespaceHandling};
use crate::nc::checks::{
    is_blank, is_semicolon_line, primary_program_end, remove_comments, rstrip_bytes, split_eot_trailer, split_lines,
    strip_bytes, ProgramEndMatcher, TAB,
};
use crate::nc::TransformResult;

/// Create transfer bytes from `data`.
///
/// A missing program end is only appended if the profile allows it **and**
/// `append_program_end_confirmed` is true (confirmation per transfer).
pub fn transform(
    data: &[u8],
    settings: &TransferSettings,
    append_program_end_confirmed: bool,
) -> Result<TransformResult, TransformError> {
    let mut applied: Vec<String> = Vec::new();
    let (body, trailer) = split_eot_trailer(data);
    let mut work = body.to_vec();
    if settings.strip_null_bytes && work.contains(&0) {
        work.retain(|b| *b != 0);
        applied.push("Nullbytes entfernt".into());
    }
    let target = settings.line_ending.sequence();
    let mut out_lines: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    let (mut removed_blank, mut removed_semicolon, mut replaced_tabs, mut trimmed, mut removed_comments) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
    let mut changed_endings = 0usize;

    for line in split_lines(&work) {
        let mut content = line.content.clone();
        if content.contains(&TAB) {
            match settings.tab_handling {
                TabHandling::Reject => {
                    return Err(TransformError(format!(
                        "Zeile {} enthält Tabulatoren (laut Profil unzulässig).",
                        line.number
                    )))
                }
                TabHandling::Replace => {
                    for b in &mut content {
                        if *b == TAB {
                            *b = b' ';
                        }
                    }
                    replaced_tabs += 1;
                }
                TabHandling::Keep => {}
            }
        }
        if settings.comment_handling == CommentHandling::Remove {
            let stripped = remove_comments(&content, &settings.comment_open, &settings.comment_close);
            if stripped != content {
                removed_comments += 1;
                if is_blank(&stripped) && !is_blank(&content) {
                    continue;
                }
                content = stripped;
            }
        }
        let new = match settings.whitespace {
            WhitespaceHandling::Trailing => rstrip_bytes(&content, b" \t").to_vec(),
            WhitespaceHandling::Both => strip_bytes(&content, b" \t").to_vec(),
            WhitespaceHandling::None => content.clone(),
        };
        if new != content {
            trimmed += 1;
            content = new;
        }
        if settings.skip_semicolon_lines && is_semicolon_line(&content) {
            removed_semicolon += 1;
            continue;
        }
        if is_blank(&content) {
            if line.ending.is_empty() && content.is_empty() {
                continue;
            }
            if settings.remove_blank_lines {
                removed_blank += 1;
                continue;
            }
        }
        let mut ending = line.ending.clone();
        if settings.normalize_line_endings && !ending.is_empty() && ending.as_slice() != target {
            ending = target.to_vec();
            changed_endings += 1;
        }
        out_lines.push((content, ending));
    }

    let normalized = format!("Zeilenenden auf {} normalisiert", settings.line_ending.as_str().to_uppercase());
    for (count, text) in [
        (replaced_tabs, "Tabulatoren durch Leerzeichen ersetzt"),
        (removed_comments, "Kommentare entfernt"),
        (trimmed, "Leerzeichen am Zeilenrand entfernt"),
        (removed_semicolon, "Semikolonzeilen entfernt"),
        (removed_blank, "Leerzeilen entfernt"),
        (changed_endings, normalized.as_str()),
    ] {
        if count > 0 {
            applied.push(format!("{text} ({count})"));
        }
    }

    let start = settings.program_start.trim();
    if !start.is_empty() {
        if !start.is_ascii() {
            return Err(TransformError(format!("Startsequenz '{start}' ist kein 7-Bit-ASCII.")));
        }
        let start_b = start.as_bytes();
        let already = out_lines.first().is_some_and(|(c, _)| strip_bytes(c, b" \t\r\n\x0b\x0c") == start_b);
        if !already {
            out_lines.insert(0, (start_b.to_vec(), target.to_vec()));
            applied.push(format!("Startsequenz '{start}' eingefügt"));
        }
    }

    let mut appended = false;
    if let Some(matcher) = ProgramEndMatcher::new(&settings.program_end) {
        if settings.append_program_end && append_program_end_confirmed {
            let has_end = out_lines
                .iter()
                .any(|(c, _)| matcher.is_match(&remove_comments(c, &settings.comment_open, &settings.comment_close)));
            if !has_end {
                if let Some(last) = out_lines.last_mut() {
                    if last.1.is_empty() {
                        last.1 = target.to_vec();
                    }
                }
                let word = primary_program_end(&settings.program_end);
                out_lines.push((word.as_bytes().to_vec(), target.to_vec()));
                appended = true;
                applied.push(format!("Programmende '{word}' ergänzt (bestätigt)"));
            }
        }
    }

    if settings.ensure_final_newline {
        if let Some(last) = out_lines.last_mut() {
            if last.1.is_empty() {
                last.1 = target.to_vec();
                applied.push("Abschließenden Zeilenumbruch ergänzt".into());
            }
        }
    }

    let mut buffer = Vec::with_capacity(work.len() + 16);
    let mut offsets = Vec::with_capacity(out_lines.len());
    for (content, ending) in &out_lines {
        buffer.extend_from_slice(content);
        buffer.extend_from_slice(ending);
        offsets.push(buffer.len());
    }
    if !trailer.is_empty() {
        if settings.send_eot_trailer {
            buffer.push(0x04);
            if trailer.contains(&b'\n') || trailer.contains(&b'\r') {
                buffer.extend_from_slice(target);
            }
            applied.push("Übertragungsende EOT am Dateiende wird mitgesendet".into());
        } else {
            applied.push("Übertragungsende EOT am Dateiende entfernt".into());
        }
    }
    if settings.encoding.eq_ignore_ascii_case("ascii") {
        if let Some((pos, byte)) = buffer.iter().enumerate().find(|(_, b)| **b > 0x7F) {
            return Err(TransformError(format!("Byte 0x{byte:02X} an Position {pos} ist kein 7-Bit-ASCII-Zeichen.")));
        }
    }
    Ok(TransformResult {
        data: buffer,
        line_count: out_lines.len(),
        line_end_offsets: offsets,
        applied,
        appended_program_end: appended,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LineEnding;

    fn settings() -> TransferSettings {
        TransferSettings { program_end: "M30,M2".into(), ..TransferSettings::default() }
    }

    #[test]
    fn normalizes_and_cleans() {
        let r = transform(b"N10 G00\tX0  \n\nN20\x00 M30", &settings(), false).unwrap();
        assert_eq!(r.data, b"N10 G00 X0\r\nN20 M30\r\n");
        assert_eq!(r.line_count, 2);
        assert_eq!(r.line_end_offsets, vec![12, 21]);
        let applied = r.applied.join("|");
        for part in ["Nullbytes", "Tabulatoren", "Leerzeichen", "Leerzeilen", "normalisiert", "Zeilenumbruch"] {
            assert!(applied.contains(part), "{part}");
        }
    }

    #[test]
    fn eot_trailer_kept_or_removed() {
        let data = b"N10 G00\r\nN20 M30\r\n\x04\r\n";
        let kept = transform(data, &settings(), false).unwrap();
        assert_eq!(kept.data, data.to_vec());
        assert_eq!(kept.line_end_offsets.last(), Some(&18));
        let off = TransferSettings { send_eot_trailer: false, ..settings() };
        assert_eq!(transform(data, &off, false).unwrap().data, b"N10 G00\r\nN20 M30\r\n");
    }

    #[test]
    fn program_end_append_needs_confirmation() {
        let allow = TransferSettings { append_program_end: true, ..settings() };
        assert_eq!(transform(b"N10 G00", &allow, false).unwrap().data, b"N10 G00\r\n");
        let r = transform(b"N10 G00", &allow, true).unwrap();
        assert_eq!(r.data, b"N10 G00\r\nM30\r\n");
        assert!(r.appended_program_end);
        assert!(!transform(b"N10 M02\r\n", &allow, true).unwrap().appended_program_end);
    }

    #[test]
    fn options() {
        let reject = TransferSettings { tab_handling: TabHandling::Reject, ..settings() };
        assert!(transform(b"A\tB\r\n", &reject, false).is_err());
        let comments = TransferSettings {
            comment_handling: CommentHandling::Remove,
            skip_semicolon_lines: true,
            whitespace: WhitespaceHandling::Both,
            line_ending: LineEnding::Lf,
            program_start: "%".into(),
            ..settings()
        };
        let r = transform(b"(KOPF)\r\n  N10 (X) G0  \r\n;SKIP\r\nM30\r\n", &comments, false).unwrap();
        assert_eq!(r.data, b"%\nN10  G0\nM30\n");
        assert!(transform(b"%\r\nN1\r\n", &comments, false).unwrap().data.starts_with(b"%\nN1"));
        let keep_ws = TransferSettings {
            whitespace: WhitespaceHandling::None,
            remove_blank_lines: false,
            normalize_line_endings: false,
            ensure_final_newline: false,
            ..settings()
        };
        assert_eq!(transform(b"A \n\nB", &keep_ws, false).unwrap().data, b"A \n\nB");
        assert!(transform("Ä\r\n".as_bytes(), &settings(), false).is_err());
        let bad_start = TransferSettings { program_start: "Ä".into(), ..settings() };
        assert!(transform(b"A\r\n", &bad_start, false).is_err());
    }
}
