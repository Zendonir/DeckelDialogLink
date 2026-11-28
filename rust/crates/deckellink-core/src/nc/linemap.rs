//! Mapping of transfer data lines back to the lines of the original file (simulator
//! follow-up of a running transfer; display only).

use crate::library::decode_ascii;

pub const SEARCH_WINDOW: usize = 200;

fn lines(data: &[u8]) -> Vec<String> {
    let text = decode_ascii(data).replace("\r\n", "\n").replace('\r', "\n");
    text.split('\n').map(|line| line.trim_matches([' ', '\t', '\0']).to_string()).collect()
}

/// `result[i]` = 1-based line number in `original` for payload line `i + 1`.
///
/// Lines are matched in order (the transformer only removes, trims or inserts lines).
/// Inserted lines map to the previous original line.
pub fn map_payload_lines(original: &[u8], payload: &[u8]) -> Vec<usize> {
    let source = lines(original);
    let mut mapping = Vec::new();
    let mut position = 0;
    let mut last = 0;
    for content in lines(payload) {
        let end = (position + SEARCH_WINDOW).min(source.len());
        let found = (position..end).find(|i| source.get(*i) == Some(&content));
        if let Some(index) = found {
            last = index + 1;
            position = index + 1;
        }
        mapping.push(last.max(1));
    }
    mapping
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_like_python() {
        let original = b"(KOPF)\r\n\r\nN10 G0  \r\nN20 G1 X5\r\n;KOMMENTAR\r\nN30 M30\r\n";
        let payload = b"%\r\n(KOPF)\r\nN10 G0\r\nN20 G1 X5\r\nN30 M30\r\n";
        assert_eq!(map_payload_lines(original, payload), [1, 1, 3, 4, 6, 7]);
    }
}
