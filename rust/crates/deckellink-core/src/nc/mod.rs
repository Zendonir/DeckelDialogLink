//! NC data: checks, analysis and transformation (originals are never modified).

pub mod analyzer;
pub mod checks;
pub mod transformer;

use crate::nc::checks::EndingCounts;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Severity::Info => "Info",
            Severity::Warning => "Warnung",
            Severity::Error => "Fehler",
        }
    }

    pub fn rank(self) -> u8 {
        match self {
            Severity::Info => 0,
            Severity::Warning => 1,
            Severity::Error => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub code: String,
    pub severity: Severity,
    pub message: String,
    pub lines: Vec<usize>,
    pub count: usize,
}

impl Finding {
    /// Up to ten line numbers plus "(+n weitere)".
    pub fn lines_text(&self) -> String {
        if self.lines.is_empty() {
            return String::new();
        }
        let shown: Vec<String> = self.lines.iter().take(10).map(ToString::to_string).collect();
        let total = if self.count == 0 { self.lines.len() } else { self.count };
        let more = total.saturating_sub(shown.len());
        let mut text = shown.join(", ");
        if more > 0 {
            text.push_str(&format!(" (+{more} weitere)"));
        }
        text
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisResult {
    pub findings: Vec<Finding>,
    pub byte_count: usize,
    pub line_count: usize,
    pub line_endings: EndingCounts,
    pub longest_line: usize,
    pub has_program_end: bool,
    pub sha256: String,
}

impl AnalysisResult {
    pub fn errors(&self) -> impl Iterator<Item = &Finding> {
        self.findings.iter().filter(|f| f.severity == Severity::Error)
    }

    pub fn warnings(&self) -> impl Iterator<Item = &Finding> {
        self.findings.iter().filter(|f| f.severity == Severity::Warning)
    }

    pub fn has_errors(&self) -> bool {
        self.errors().next().is_some()
    }

    pub fn has_warnings(&self) -> bool {
        self.warnings().next().is_some()
    }

    pub fn has(&self, code: &str) -> bool {
        self.findings.iter().any(|f| f.code == code)
    }

    pub fn get(&self, code: &str) -> Option<&Finding> {
        self.findings.iter().find(|f| f.code == code)
    }
}

/// Prepared transfer bytes. Never written over the original file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TransformResult {
    pub data: Vec<u8>,
    pub line_count: usize,
    pub line_end_offsets: Vec<usize>,
    pub applied: Vec<String>,
    pub appended_program_end: bool,
}

impl TransformResult {
    /// 1-based line number containing byte `offset` (bytes already sent).
    pub fn line_for_offset(&self, offset: usize) -> usize {
        if self.line_end_offsets.is_empty() {
            return 0;
        }
        let index = self.line_end_offsets.partition_point(|end| *end < offset);
        (index + 1).min(self.line_end_offsets.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finding_lines_text() {
        let f = Finding {
            code: "X".into(),
            severity: Severity::Warning,
            message: String::new(),
            lines: (1..=12).collect(),
            count: 15,
        };
        assert_eq!(f.lines_text(), "1, 2, 3, 4, 5, 6, 7, 8, 9, 10 (+5 weitere)");
        let empty = Finding { lines: vec![], ..f };
        assert_eq!(empty.lines_text(), "");
        assert!(Severity::Error > Severity::Warning && Severity::Error.rank() == 2);
    }

    #[test]
    fn line_for_offset() {
        let t = TransformResult { line_end_offsets: vec![5, 10, 15], ..TransformResult::default() };
        assert_eq!(t.line_for_offset(0), 1);
        assert_eq!(t.line_for_offset(5), 1);
        assert_eq!(t.line_for_offset(6), 2);
        assert_eq!(t.line_for_offset(99), 3);
        assert_eq!(TransformResult::default().line_for_offset(3), 0);
    }
}
