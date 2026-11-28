//! Lexical structure of DIALOG 11 program text.
//!
//! Observed syntax (training documentation, not a formal specification):
//!
//! * block `N10 G1 X10 YI-5 F200; M3` - several sub-blocks separated by `;`,
//! * word = address letters + value; `A`/`I` after a coordinate address means
//!   absolute/incremental for this word (`XI-40`, `XA50`),
//! * values may be parameters: `XP1`, `Z-P3`, `XPP4`, `XPT3`,
//! * G functions with a variant: `G87 • 2` (written here as `G87.2`, `G87*2` or `G87+2`),
//! * subprograms `%*1` (of a main program), `%%4` (macro), `%%*1`,
//! * parameter assignment `P1 = (P2 + 5)`, calculation `P4!`,
//!   `IF P1 >= 3 THEN L0 N90 ELSE G0 Z100`,
//! * comments in round brackets.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use regex::{Captures, Regex};

/// Addresses that also exist with the A (absolute) / I (incremental) suffix.
const BASE_SUFFIXABLE: [&str; 22] = [
    "X", "Y", "Z", "C", "I", "J", "K", "PX", "PY", "PZ", "PW", "W", "O", "T", "S", "E", "LX", "LY", "BW", "EW", "M",
    "D",
];
const PLAIN: [&str; 43] = [
    "N", "G", "F", "TT", "L", "R", "PL", "UW", "UR", "FA", "AM", "FE", "FZ", "LI", "BI", "DW", "RL", "RB", "RT", "DR",
    "DN", "ST", "LL", "LW", "LA", "LB", "LC", "BL", "BX", "BY", "BA", "BB", "BC", "B", "SL", "SH", "AW", "TW", "A",
    "H", "Q", "U", "V",
];
const PARAM_SUFFIXES: [&str; 4] = ["PPT", "PP", "PT", "P"];

/// All known address names.
pub fn addresses() -> &'static BTreeSet<String> {
    static SET: OnceLock<BTreeSet<String>> = OnceLock::new();
    SET.get_or_init(|| {
        let mut set: BTreeSet<String> = PLAIN.iter().chain(BASE_SUFFIXABLE.iter()).map(|a| (*a).to_string()).collect();
        for base in BASE_SUFFIXABLE {
            set.insert(format!("{base}A"));
            set.insert(format!("{base}I"));
        }
        set
    })
}

fn is_address(name: &str) -> bool {
    addresses().contains(name)
}

macro_rules! regex {
    ($name:ident, $pattern:expr) => {
        fn $name() -> Option<&'static Regex> {
            static RE: OnceLock<Option<Regex>> = OnceLock::new();
            RE.get_or_init(|| Regex::new($pattern).ok()).as_ref()
        }
    };
}

regex!(comment_re, r"\([^()]*\)");
regex!(g_variant_re, r"G\s*([0-9]+)\s*[.*+•·∙×]\s*([0-9]+)");
regex!(sub_re, r"(%%?)\s*([*+•·∙×.])?\s*([0-9]+)");
regex!(word_re, r"^([A-Z]+)([-+]?(?:PPT|PP|PT|P)[0-9]+|[-+]?(?:[0-9]+\.?[0-9]*|\.[0-9]+))?");
regex!(assign_re, r"^(PP|P)([0-9]+)\s*=\s*(.*)$");
regex!(calc_re, r"^(PP|P)([0-9]+)\s*!$");
regex!(if_re, r"^IF\s*(.+?)\s*THEN\s*(.+?)(?:\s*ELSE\s*(.+))?$");
regex!(header_re, r"^\s*(%%?)\s*([*+•·∙×.])?\s*([0-9]+)\s*$");
regex!(expression_line_re, r"(?:^|[;\s])P{1,2}[0-9]+\s*=|(?:^|[;\s0-9])IF\b");
regex!(variant_mark_re, r"G([0-9]+)#([0-9]+)");
regex!(whitespace_re, r"\s+");
regex!(number_re, r"^\s*N\s*([0-9]+)");
regex!(if_start_re, r"^\s*IF");

/// Digits to integer (saturating like an unbounded Python int would never be needed).
pub fn parse_int(digits: &str) -> i64 {
    digits.parse::<i64>().unwrap_or(i64::MAX)
}

fn group<'t>(caps: &Captures<'t>, index: usize) -> &'t str {
    caps.get(index).map_or("", |m| m.as_str())
}

/// A sub-block could not be tokenized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxProblem(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RefKind {
    Sub,
    Macro,
    MacroSub,
    Main,
}

/// `%*1` -> (Sub, 1); `%%4` -> (Macro, 4); `%%*1` -> (MacroSub, 1); `%3` -> (Main, 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SubprogramRef {
    pub kind: RefKind,
    pub number: i64,
}

impl SubprogramRef {
    pub fn label(&self) -> String {
        let n = self.number;
        match self.kind {
            RefKind::Sub => format!("%*{n}"),
            RefKind::Macro => format!("%%{n}"),
            RefKind::MacroSub => format!("%%*{n}"),
            RefKind::Main => format!("%{n}"),
        }
    }
}

pub fn subprogram_ref(percent: &str, marker: bool, number: &str) -> SubprogramRef {
    let kind = match (percent == "%%", marker) {
        (true, true) => RefKind::MacroSub,
        (true, false) => RefKind::Macro,
        (false, true) => RefKind::Sub,
        (false, false) => RefKind::Main,
    };
    SubprogramRef { kind, number: parse_int(number) }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    pub address: String,
    /// Number text, parameter reference ("-P3") or "" (no value).
    pub value: String,
    /// G87.2 -> address "G", value "87", variant 2.
    pub variant: Option<i64>,
}

impl Word {
    pub fn new(address: &str, value: &str) -> Self {
        Self { address: address.to_string(), value: value.to_string(), variant: None }
    }

    pub fn is_parameter(&self) -> bool {
        self.value.contains('P')
    }
}

impl std::fmt::Display for Word {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}", self.address, self.value)?;
        if let Some(variant) = self.variant {
            write!(f, ".{variant}")?;
        }
        Ok(())
    }
}

/// One statement of a block (blocks may hold several, separated by ';').
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Statement {
    Words { words: Vec<Word>, call: Option<SubprogramRef> },
    Assign { target: String, expression: String },
    Calc { target: String },
    If { condition: String, then_text: String, else_text: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// 1-based line number in the source text.
    pub line: usize,
    /// Original line.
    pub text: String,
    /// N number.
    pub number: Option<i64>,
    pub statements: Vec<Statement>,
    pub problem: String,
}

pub fn strip_comments(text: &str) -> String {
    let mut current = text.to_string();
    let Some(re) = comment_re() else { return current };
    loop {
        let next = re.replace_all(&current, " ").into_owned();
        if next == current {
            return current;
        }
        current = next;
    }
}

/// Upper-case, comments removed, G variants written as `G87#2`.
///
/// Brackets in parameter assignments and IF statements are arithmetic, not comments.
pub fn normalize(text: &str) -> String {
    let mut upper = text.to_uppercase();
    if !expression_line_re().is_some_and(|re| re.is_match(&upper)) {
        upper = strip_comments(&upper);
    }
    let text = upper.replace('\t', " ");
    match g_variant_re() {
        Some(re) => re.replace_all(&text, "G${1}#${2}").into_owned(),
        None => text,
    }
}

/// Split a sub-block into words; `%` references are returned separately.
pub fn tokenize_words(text: &str) -> Result<(Vec<Word>, Option<SubprogramRef>), SyntaxProblem> {
    let mut call = None;
    let mut text = text.to_string();
    if let Some(caps) = sub_re().and_then(|re| re.captures(&text)) {
        call = Some(subprogram_ref(group(&caps, 1), caps.get(2).is_some(), group(&caps, 3)));
        if let Some(whole) = caps.get(0) {
            let (start, end) = (whole.start(), whole.end());
            text = format!("{} {}", text.get(..start).unwrap_or(""), text.get(end..).unwrap_or(""));
        }
    }
    let compact = match whitespace_re() {
        Some(re) => re.replace_all(&text, "").into_owned(),
        None => text,
    };
    let mut words = Vec::new();
    let mut rest = compact.as_str();
    while let Some(first) = rest.chars().next() {
        let Some(caps) = word_re().and_then(|re| re.captures(rest)) else {
            return Err(SyntaxProblem(format!("Unerwartetes Zeichen '{first}'")));
        };
        let end = caps.get(0).map_or(0, |m| m.end());
        words.push(make_word(group(&caps, 1), group(&caps, 2))?);
        rest = rest.get(end..).unwrap_or("");
    }
    Ok((words, call))
}

fn make_word(letters: &str, value: &str) -> Result<Word, SyntaxProblem> {
    if letters == "G" || is_address(letters) || matches!(letters, "P" | "PP" | "PT" | "PPT") {
        return Ok(Word::new(letters, value));
    }
    // "XP1" = X with parameter P1, "TIPP2" = TI with PP2
    let first = value.chars().next();
    for suffix in PARAM_SUFFIXES {
        let Some(head) = letters.strip_suffix(suffix) else { continue };
        if !is_address(head) {
            continue;
        }
        match first {
            Some(c) if c.is_ascii_digit() => return Ok(Word::new(head, &format!("{suffix}{value}"))),
            Some(c @ ('+' | '-')) => {
                return Ok(Word::new(head, &format!("{c}{suffix}{}", value.get(1..).unwrap_or(""))));
            }
            _ => {}
        }
    }
    Err(SyntaxProblem(format!("Unbekannte Adresse '{letters}'")))
}

pub fn parse_statement(text: &str) -> Result<Option<Statement>, SyntaxProblem> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    if let Some(caps) = assign_re().and_then(|re| re.captures(text)) {
        return Ok(Some(Statement::Assign {
            target: format!("{}{}", group(&caps, 1), group(&caps, 2)),
            expression: group(&caps, 3).trim().to_string(),
        }));
    }
    if let Some(caps) = calc_re().and_then(|re| re.captures(text)) {
        return Ok(Some(Statement::Calc { target: format!("{}{}", group(&caps, 1), group(&caps, 2)) }));
    }
    if let Some(caps) = if_re().and_then(|re| re.captures(text)) {
        return Ok(Some(Statement::If {
            condition: group(&caps, 1).trim().to_string(),
            then_text: group(&caps, 2).trim().to_string(),
            else_text: group(&caps, 3).trim().to_string(),
        }));
    }
    let (words, call) = tokenize_with_variants(text)?;
    Ok(Some(Statement::Words { words, call }))
}

fn tokenize_with_variants(text: &str) -> Result<(Vec<Word>, Option<SubprogramRef>), SyntaxProblem> {
    let mut variants: Vec<(String, i64)> = Vec::new();
    let plain = match variant_mark_re() {
        Some(re) => re
            .replace_all(text, |caps: &Captures<'_>| {
                variants.push((group(caps, 1).to_string(), parse_int(group(caps, 2))));
                format!("G{}\u{0}", group(caps, 1))
            })
            .into_owned(),
        None => text.to_string(),
    };
    // "\0" marks a G word with variant; split there to keep words separate
    let mut words: Vec<Word> = Vec::new();
    let mut call = None;
    for (index, part) in plain.split('\u{0}').enumerate() {
        let (part_words, part_call) = tokenize_words(part)?;
        call = call.or(part_call);
        words.extend(part_words);
        if let Some((number, variant)) = variants.get(index) {
            // the last G word of this part carries the variant
            if let Some(word) = words.iter_mut().rev().find(|w| w.address == "G" && &w.value == number) {
                word.variant = Some(*variant);
            }
        }
    }
    Ok((words, call))
}

/// Parse one program line (comments removed, `;` sub-blocks split).
pub fn parse_line(line_no: usize, raw: &str) -> Block {
    let mut text = normalize(raw);
    let mut number = None;
    if let Some(caps) = number_re().and_then(|re| re.captures(&text)) {
        number = Some(parse_int(group(&caps, 1)));
        let end = caps.get(0).map_or(0, |m| m.end());
        text = text.get(end..).unwrap_or("").to_string();
    }
    let mut statements = Vec::new();
    let parts: Vec<&str> = if if_start_re().is_some_and(|re| re.is_match(&text)) {
        vec![text.as_str()]
    } else {
        text.split(';').collect()
    };
    for part in parts {
        match parse_statement(part) {
            Ok(Some(statement)) => statements.push(statement),
            Ok(None) => {}
            Err(problem) => {
                return Block { line: line_no, text: raw.to_string(), number, statements, problem: problem.0 };
            }
        }
    }
    Block { line: line_no, text: raw.to_string(), number, statements, problem: String::new() }
}

/// A main program, macro or subprogram as a list of blocks.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProgramText {
    /// None = main program of the file.
    pub reference: Option<SubprogramRef>,
    pub blocks: Vec<Block>,
}

impl ProgramText {
    pub fn label(&self) -> String {
        self.reference.map_or_else(|| "Hauptprogramm".to_string(), |r| r.label())
    }

    pub fn index_of(&self, number: i64) -> Option<usize> {
        self.blocks.iter().position(|b| b.number == Some(number))
    }
}

/// Split a file into the main program and further programs (in file order).
///
/// A line holding only `%*1`, `%%4` ... (without N number) opens a new program
/// section; this file layout is a simulator convention. Lines that continue an IF
/// statement with `ELSE` are joined to the previous line.
pub fn split_programs(text: &str) -> (ProgramText, Vec<ProgramText>) {
    let mut main = ProgramText::default();
    let mut others: Vec<ProgramText> = Vec::new();
    // None = main program, Some(i) = others[i]
    let mut current: Option<usize> = None;
    let mut pending: Option<(usize, String)> = None;
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");

    let flush = |pending: &mut Option<(usize, String)>,
                 current: Option<usize>,
                 main: &mut ProgramText,
                 others: &mut Vec<ProgramText>| {
        if let Some((line, raw)) = pending.take() {
            let block = parse_line(line, &raw);
            match current.and_then(|i| others.get_mut(i)) {
                Some(program) => program.blocks.push(block),
                None => main.blocks.push(block),
            }
        }
    };

    for (index, raw) in normalized.split('\n').enumerate() {
        let clean = raw.replace('\u{4}', "");
        let clean = clean.trim_end();
        let stripped_owned = strip_comments(clean);
        let stripped = stripped_owned.trim();
        if stripped.is_empty() {
            continue;
        }
        if let Some(caps) = header_re().and_then(|re| re.captures(stripped)) {
            flush(&mut pending, current, &mut main, &mut others);
            let reference = subprogram_ref(group(&caps, 1), caps.get(2).is_some(), group(&caps, 3));
            if reference.kind == RefKind::Main && main.blocks.is_empty() && current.is_none() {
                continue; // "%1" at the top names the main program
            }
            let program = ProgramText { reference: Some(reference), blocks: Vec::new() };
            if let Some(position) = others.iter().position(|p| p.reference == Some(reference)) {
                if let Some(slot) = others.get_mut(position) {
                    *slot = program;
                }
                current = Some(position);
            } else {
                others.push(program);
                current = Some(others.len() - 1);
            }
            continue;
        }
        if stripped == "%" || stripped == "%%" {
            continue;
        }
        if stripped.to_uppercase().starts_with("ELSE") {
            if let Some((_, text)) = pending.as_mut() {
                text.push(' ');
                text.push_str(clean);
                continue;
            }
        }
        flush(&mut pending, current, &mut main, &mut others);
        pending = Some((index + 1, clean.to_string()));
    }
    flush(&mut pending, current, &mut main, &mut others);
    (main, others)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<String> {
        match parse_statement(&normalize(text)) {
            Ok(Some(Statement::Words { words, .. })) => words.iter().map(ToString::to_string).collect(),
            other => vec![format!("{other:?}")],
        }
    }

    #[test]
    fn tokenizes_like_python() {
        assert_eq!(words("G1 XI-40 YP1 Z-P3 XPP4"), ["G1", "XI-40", "YP1", "Z-P3", "XPP4"]);
        assert_eq!(words("G87 • 2 D30 AM4"), ["G87.2", "D30", "AM4"]);
        assert_eq!(words("TIPP2"), ["TIPP2"]);
        let block = parse_line(3, "N20 G17 T1 M6; M3 S2000 (KOMMENTAR)");
        assert_eq!(block.number, Some(20));
        assert_eq!(block.statements.len(), 2);
        let block = parse_line(1, "N5 G1 Q? X1");
        assert_eq!(block.problem, "Unerwartetes Zeichen '?'");
        let block = parse_line(1, "N5 XYZ1");
        assert_eq!(block.problem, "Unbekannte Adresse 'XYZ'");
        assert!(matches!(
            parse_statement("IF P1 < 5 THEN L0 N190 ELSE G0 Z100"),
            Ok(Some(Statement::If { ref else_text, .. })) if else_text == "G0 Z100"
        ));
    }

    #[test]
    fn splits_programs() {
        let (main, others) = split_programs("%1\nN10 G0 X1\nIF P1 = 1 THEN G0 X2\nELSE G0 X3\n%*1\nN10 M17\n");
        assert_eq!(main.blocks.len(), 2);
        assert_eq!(others.len(), 1);
        assert_eq!(others.first().map(ProgramText::label).as_deref(), Some("%*1"));
    }
}
