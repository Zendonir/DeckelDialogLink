//! Execution of DIALOG 11 program text into tool paths (offline simulation).

use std::cell::Cell;
use std::collections::{BTreeMap, HashSet};
use std::rc::Rc;
use std::sync::OnceLock;

use regex::Regex;

use crate::sim::cycles;
use crate::sim::expressions::{evaluate, is_arithmetic, parse_float, ExpressionError};
use crate::sim::geometry::{
    arc_points, axis, axis_name, circle_center_from_radius, direction, distance, offset_polyline, plane_for, set_axis,
    Plane, Transform, Vec2, Vec3, G17,
};
use crate::sim::model::{Dwell, Message, Move, MoveKind, Severity, SimConfig, SimResult, Snapshot, ToolData};
use crate::sim::syntax::{parse_statement, split_programs, ProgramText, Statement, SubprogramRef, SyntaxProblem, Word};
use crate::sim::{py_g, py_int};

/// Nesting limit for parameter references (the Python version would hit its recursion limit).
const MAX_RESOLVE_DEPTH: usize = 64;

/// Control flow of the interpreter (exceptions of the Python version).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Flow {
    /// Program end or abort.
    Stop(String),
    /// M17 - leave the current subprogram.
    Return,
    /// Jump to an N number.
    Jump(i64),
    /// Statement error (expression, syntax or value problem); reported, block continues.
    Error(String),
    /// Cycle block incomplete or inconsistent; reported by the cycle runner.
    Cycle(String),
}

impl From<ExpressionError> for Flow {
    fn from(error: ExpressionError) -> Self {
        Self::Error(error.0)
    }
}

impl From<SyntaxProblem> for Flow {
    fn from(problem: SyntaxProblem) -> Self {
        Self::Error(problem.0)
    }
}

pub type Run<T = ()> = Result<T, Flow>;

pub fn known_m(m: i64) -> Option<&'static str> {
    Some(match m {
        2 => "Programmende",
        3 => "Spindel rechts",
        4 => "Spindel links",
        5 => "Spindel halt",
        6 => "Werkzeugwechsel",
        17 => "Unterprogramm-Ende",
        30 => "Programmende mit Rücksprung",
        60 => "konstanter Vorschub an der Kontur",
        61 => "konstanter Vorschub an Innenradien",
        62 => "konstanter Vorschub des Fräsermittelpunkts",
        79 => "Ende Konturtasche",
        _ => return None,
    })
}

fn corner_mode(g: i64, variant: Option<i64>) -> Option<&'static str> {
    match (g, variant) {
        (60, None) => Some("G60 Übergangskreis"),
        (60, Some(1)) => Some("G60.1 Genauhalt"),
        (60, Some(2)) => Some("G60.2 verkürzter Weg"),
        (64, None) => Some("G64 Verschleifen innen"),
        (64, Some(1)) => Some("G64.1 Verschleifen innen+außen"),
        _ => None,
    }
}

const COORD_AXES: [&str; 3] = ["X", "Y", "Z"];
const CENTER_AXES: [&str; 3] = ["I", "J", "K"];
const POLAR_POLE: [&str; 3] = ["PX", "PY", "PZ"];
const SUFFIXES: [&str; 3] = ["", "A", "I"];

fn compare_re() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(.*?)(>=|<=|<>|==|=|>|<)(.*)$").ok()).as_ref()
}

fn parameter_name_re() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(?:PP|P)[0-9]+$").ok()).as_ref()
}

/// Parameter memory value: number or block content.
#[derive(Debug, Clone, PartialEq)]
pub enum Memory {
    Number(f64),
    Text(String),
}

/// Words of a block by address (last word wins).
pub type ByAddress<'a> = BTreeMap<&'a str, &'a Word>;

pub fn by_address(words: &[Word]) -> ByAddress<'_> {
    words.iter().map(|w| (w.address.as_str(), w)).collect()
}

/// Contour element collected while radius compensation is active.
#[derive(Debug, Clone)]
struct Element {
    kind: MoveKind,
    /// Workpiece coordinates, without start point.
    points: Vec<Vec3>,
    line: usize,
    program: String,
    feed: f64,
}

#[derive(Debug, Clone)]
struct Compensation {
    left: bool,
    approach: Option<i64>,
    approach_r: f64,
    tool_start: Vec3,
    entry_kind: MoveKind,
    entry_line: usize,
    entry_program: String,
    contour_start: Vec3,
    elements: Vec<Element>,
}

#[derive(Debug, Clone)]
struct Context {
    program: Rc<ProgramText>,
    label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Track {
    Moves,
    Programmed,
}

/// Runs a program text and records moves, states and messages.
#[derive(Debug)]
pub struct Interpreter {
    pub(crate) config: SimConfig,
    pub(crate) result: SimResult,
    main: Rc<ProgramText>,
    programs: Vec<Rc<ProgramText>>,
    pub(crate) plane: Plane,
    absolute: bool,
    motion: i64,
    pub(crate) rapid_variant: i64,
    comp_mode: i64,
    corner_mode: String,
    feed_mode: String,
    base: Vec3,
    base_label: String,
    shift: Vec3,
    rotation: f64,
    mirror: [Option<f64>; 3],
    scale: Vec3,
    tool_label: String,
    pub(crate) tool: ToolData,
    feed: f64,
    pub(crate) speed: f64,
    spindle: String,
    c_axis: f64,
    other_m: Vec<i64>,
    tool_w: Vec3,
    contour_w: Vec3,
    memory: Vec<(String, Memory)>,
    comp: Option<Compensation>,
    /// (depth level, retract level) in the tool axis.
    pub(crate) contour_pocket: Option<(f64, f64)>,
    blocks_run: usize,
    call_depth: usize,
    points_total: usize,
    ctx: Context,
    line: usize,
    warned: HashSet<String>,
    feed_warned: bool,
    resolve_depth: Cell<usize>,
}

impl Interpreter {
    pub fn new(text: &str, config: SimConfig) -> Self {
        let (main, others) = split_programs(text);
        let main = Rc::new(main);
        let programs: Vec<Rc<ProgramText>> = others.into_iter().map(Rc::new).collect();
        let result = SimResult {
            program_labels: std::iter::once(main.label()).chain(programs.iter().map(|p| p.label())).collect(),
            ..SimResult::default()
        };
        let start = config.start_position;
        let ctx = Context { program: Rc::clone(&main), label: main.label() };
        Self {
            config,
            result,
            main,
            programs,
            plane: G17,
            absolute: true,
            motion: 0,
            rapid_variant: 0,
            comp_mode: 40,
            corner_mode: "G60 Übergangskreis".into(),
            feed_mode: "M60".into(),
            base: [0.0; 3],
            base_label: "G51.0".into(),
            shift: [0.0; 3],
            rotation: 0.0,
            mirror: [None; 3],
            scale: [1.0; 3],
            tool_label: "T0".into(),
            tool: ToolData::with_radius(0.0),
            feed: 0.0,
            speed: 0.0,
            spindle: "M5".into(),
            c_axis: 0.0,
            other_m: Vec::new(),
            tool_w: start,
            contour_w: start,
            memory: Vec::new(),
            comp: None,
            contour_pocket: None,
            blocks_run: 0,
            call_depth: 0,
            points_total: 0,
            ctx,
            line: 0,
            warned: HashSet::new(),
            feed_warned: false,
            resolve_depth: Cell::new(0),
        }
    }

    // ------------------------------------------------------------------ public
    pub fn run(mut self) -> SimResult {
        self.record_state();
        let main = Rc::clone(&self.main);
        match self.run_program(main) {
            Ok(()) => {
                self.result.end_reason = "Programmende ohne M30/M2 erreicht".into();
                self.msg_at(Severity::Warning, "Programm endet ohne M30 oder M2.", Some(0), false);
            }
            Err(Flow::Stop(reason)) => self.result.end_reason = reason,
            Err(Flow::Error(text) | Flow::Cycle(text)) => {
                self.msg(Severity::Error, &text);
                self.result.end_reason = format!("Abbruch: {text}");
            }
            Err(Flow::Return | Flow::Jump(_)) => self.result.end_reason = "Abbruch: unerwarteter Programmfluss".into(),
        }
        if self.comp.is_some() {
            self.warn("Bahnkorrektur am Programmende nicht mit G40 abgewählt.");
            if let Err(Flow::Stop(reason)) = self.flush_compensation(None, None) {
                self.result.end_reason = reason;
            }
        }
        self.result.executed_blocks = self.blocks_run;
        self.result.parameters = self
            .memory
            .iter()
            .map(|(k, v)| {
                let text = match v {
                    Memory::Number(n) => py_g(*n),
                    Memory::Text(t) => t.clone(),
                };
                (k.clone(), text)
            })
            .collect();
        self.result
    }

    // ------------------------------------------------------------------ messages / state
    fn msg_at(&mut self, severity: Severity, text: &str, line: Option<usize>, once: bool) {
        if once && !self.warned.insert(text.to_string()) {
            return;
        }
        let line = line.unwrap_or(self.line);
        self.result.messages.push(Message { severity, text: text.to_string(), line, program: self.ctx.label.clone() });
    }

    pub(crate) fn msg(&mut self, severity: Severity, text: &str) {
        self.msg_at(severity, text, None, false);
    }

    /// Simplification note (shown once per text).
    pub(crate) fn note(&mut self, text: &str) {
        self.msg_at(Severity::Info, text, None, true);
    }

    pub(crate) fn warn(&mut self, text: &str) {
        self.msg(Severity::Warning, text);
    }

    pub(crate) fn transform(&self) -> Transform {
        Transform {
            offset: [self.base[0] + self.shift[0], self.base[1] + self.shift[1], self.base[2] + self.shift[2]],
            rotation_deg: self.rotation,
            plane: self.plane,
            mirror: self.mirror,
            scale: self.scale,
        }
    }

    /// Programmed position in the active program coordinate system.
    pub(crate) fn position(&self) -> Vec3 {
        self.transform().invert(self.contour_w)
    }

    fn modal(&self) -> Vec<(String, String)> {
        let mut motion = format!("G{}", self.motion);
        if self.motion == 0 && self.rapid_variant != 0 {
            motion.push_str(&format!(".{}", self.rapid_variant));
        }
        let comp = match self.comp_mode {
            41 => "G41 links",
            42 => "G42 rechts",
            _ => "G40 aus",
        };
        let mut zero = self.base_label.clone();
        if self.shift.iter().any(|v| v.abs() > 1e-9) {
            zero.push_str(&format!(
                " + Verschiebung {}/{}/{}",
                py_g(self.shift[0]),
                py_g(self.shift[1]),
                py_g(self.shift[2])
            ));
        }
        if self.rotation.abs() > 1e-9 {
            zero.push_str(&format!(", Drehung {}°", py_g(self.rotation)));
        }
        let mirror: Vec<String> = self
            .mirror
            .iter()
            .enumerate()
            .filter_map(|(i, m)| m.map(|m| format!("{}{}", axis_name(i), py_g(m))))
            .collect();
        #[allow(clippy::float_cmp)]
        let scale: Vec<String> = self
            .scale
            .iter()
            .enumerate()
            .filter(|(_, s)| **s != 1.0)
            .map(|(i, s)| format!("{}{}", axis_name(i), py_g(*s)))
            .collect();
        let join = |items: Vec<String>| if items.is_empty() { "aus".to_string() } else { items.join(", ") };
        let spindle_m = self.spindle.get(1..).and_then(|t| t.parse::<i64>().ok()).unwrap_or(5);
        let spindle = format!("{} {}", self.spindle, known_m(spindle_m).unwrap_or("")).trim().to_string();
        let mut items: Vec<(String, String)> = vec![
            ("Wegbedingung".into(), motion),
            ("Ebene".into(), self.plane.label()),
            ("Maßangabe".into(), if self.absolute { "G90 absolut" } else { "G91 inkremental" }.into()),
            ("Bahnkorrektur".into(), comp.into()),
            ("Eckenverhalten".into(), self.corner_mode.clone()),
            ("Vorschubverhalten".into(), self.feed_mode.clone()),
            ("Nullpunkt".into(), zero),
            ("Spiegeln G58".into(), join(mirror)),
            ("Maßstab G59".into(), join(scale)),
            ("Spindel".into(), spindle),
        ];
        if self.contour_pocket.is_some() {
            items.push(("Zyklus".into(), "G87.9 Konturtasche aktiv".into()));
        }
        if !self.other_m.is_empty() {
            let skip = self.other_m.len().saturating_sub(6);
            let list: Vec<String> = self.other_m.iter().skip(skip).map(|m| format!("M{m}")).collect();
            items.push(("Weitere M".into(), list.join(" ")));
        }
        items
    }

    fn record_state(&mut self) -> usize {
        let snapshot = Snapshot {
            line: self.line,
            program: self.ctx.label.clone(),
            position: self.position(),
            modal: self.modal(),
            tool: self.tool_label.clone(),
            tool_radius: self.tool.radius,
            feed: self.feed,
            speed: self.speed,
            spindle: self.spindle.clone(),
            c_axis: self.c_axis,
        };
        self.result.snapshots.push(snapshot);
        self.result.snapshots.len() - 1
    }

    // ------------------------------------------------------------------ program flow
    fn run_program(&mut self, program: Rc<ProgramText>) -> Run {
        let label = program.label();
        let next = Context { program: Rc::clone(&program), label: label.clone() };
        let previous = std::mem::replace(&mut self.ctx, next);
        let result = self.run_range(0, program.blocks.len());
        self.ctx = previous;
        match result {
            Err(Flow::Return) => Ok(()),
            Err(Flow::Jump(number)) => Err(Flow::Stop(format!("Sprungziel N{number} nicht im Programm {label}"))),
            other => other,
        }
    }

    fn run_range(&mut self, start: usize, end: usize) -> Run {
        let program = Rc::clone(&self.ctx.program);
        let mut pc = start;
        while pc < end {
            let Some(block) = program.blocks.get(pc) else { break };
            match self.run_block(block) {
                Ok(()) => pc += 1,
                Err(Flow::Jump(number)) => {
                    let Some(target) = program.index_of(number) else {
                        return Err(Flow::Stop(format!("Sprungziel N{number} nicht gefunden")));
                    };
                    pc = target;
                    if pc >= end || pc < start {
                        return Err(Flow::Jump(number));
                    }
                }
                Err(other) => return Err(other),
            }
        }
        Ok(())
    }

    fn run_block(&mut self, block: &crate::sim::syntax::Block) -> Run {
        self.blocks_run += 1;
        if self.blocks_run > self.config.max_blocks {
            self.line = block.line;
            let text = format!("Mehr als {} Sätze ausgeführt - Endlosschleife?", self.config.max_blocks);
            self.msg(Severity::Error, &text);
            return Err(Flow::Stop("Abbruch: zu viele Sätze (Endlosschleife?)".into()));
        }
        self.line = block.line;
        if !block.problem.is_empty() {
            let text = format!("Satz nicht lesbar: {}", block.problem);
            self.msg(Severity::Error, &text);
            return Ok(());
        }
        let mut stop = None;
        let mut returning = false;
        for statement in &block.statements {
            match self.run_statement(statement) {
                Ok(()) => {}
                Err(Flow::Stop(reason)) => {
                    stop = Some(reason);
                    break;
                }
                Err(Flow::Return) => {
                    returning = true;
                    break;
                }
                Err(Flow::Error(text) | Flow::Cycle(text)) => self.msg(Severity::Error, &text),
                Err(jump @ Flow::Jump(_)) => return Err(jump),
            }
        }
        self.record_state();
        if let Some(reason) = stop {
            return Err(Flow::Stop(reason));
        }
        if returning {
            return Err(Flow::Return);
        }
        Ok(())
    }

    pub(crate) fn run_statement(&mut self, statement: &Statement) -> Run {
        match statement {
            Statement::Assign { target, expression } => self.assign(target, expression),
            Statement::Calc { target } => self.calculate(target),
            Statement::If { condition, then_text, else_text } => {
                let branch = if self.condition(condition)? { then_text } else { else_text };
                self.run_text_statements(branch)
            }
            Statement::Words { words, call } => self.run_words(words, *call),
        }
    }

    fn run_text_statements(&mut self, text: &str) -> Run {
        for part in text.split(';') {
            if let Some(sub) = parse_statement(part)? {
                self.run_statement(&sub)?;
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------------ parameters
    pub(crate) fn resolve(&self, name: &str) -> Result<f64, ExpressionError> {
        let depth = self.resolve_depth.get();
        if depth >= MAX_RESOLVE_DEPTH {
            return Err(ExpressionError("Parameter-Verweise zu tief verschachtelt".into()));
        }
        self.resolve_depth.set(depth + 1);
        let result = match self.lookup(name) {
            Ok(Memory::Number(value)) => Ok(value),
            Ok(Memory::Text(text)) if is_arithmetic(&text) => evaluate(&text, &mut |n: &str| self.resolve(n)),
            Ok(Memory::Text(text)) => Err(ExpressionError(format!("{name} enthält keinen Zahlenwert ('{text}')"))),
            Err(error) => Err(error),
        };
        self.resolve_depth.set(depth);
        result
    }

    fn lookup(&self, name: &str) -> Result<Memory, ExpressionError> {
        let table_only = name.starts_with("PT") || name.starts_with("PPT");
        let key = if table_only { name.replace("PPT", "PP").replace("PT", "P") } else { name.to_string() };
        if !table_only {
            if let Some((_, value)) = self.memory.iter().find(|(k, _)| *k == key) {
                return Ok(value.clone());
            }
        }
        if let Some(text) = self.config.parameter_table.get(&key) {
            return Ok(Memory::Text(text.clone()));
        }
        Err(ExpressionError(format!("Parameter {key} ist nicht belegt")))
    }

    fn store(&mut self, target: &str, value: Memory) {
        match self.memory.iter_mut().find(|(k, _)| k == target) {
            Some((_, slot)) => *slot = value,
            None => self.memory.push((target.to_string(), value)),
        }
    }

    fn evaluate(&self, text: &str) -> Result<f64, ExpressionError> {
        evaluate(text, &mut |n: &str| self.resolve(n))
    }

    fn assign(&mut self, target: &str, expression: &str) -> Run {
        let value = if is_arithmetic(expression) {
            Memory::Number(self.evaluate(expression)?)
        } else {
            Memory::Text(expression.trim().to_string())
        };
        self.store(target, value);
        Ok(())
    }

    fn calculate(&mut self, target: &str) -> Run {
        let Some(text) = self.config.parameter_table.get(target).cloned() else {
            return Err(Flow::Error(format!("{target}! - keine Zuweisung in der Parametertabelle")));
        };
        let value = self.evaluate(&text)?;
        self.store(target, Memory::Number(value));
        Ok(())
    }

    fn condition(&self, text: &str) -> Result<bool, ExpressionError> {
        let compact = text.replace(' ', "");
        let caps = compare_re()
            .and_then(|re| re.captures(&compact))
            .ok_or_else(|| ExpressionError(format!("IF-Bedingung nicht lesbar: {text}")))?;
        let part = |i: usize| caps.get(i).map_or("", |m| m.as_str());
        let left = self.evaluate(part(1))?;
        let right = self.evaluate(part(3))?;
        Ok(match part(2) {
            ">" => left > right,
            "<" => left < right,
            ">=" => left >= right - 1e-9,
            "<=" => left <= right + 1e-9,
            "<>" => (left - right).abs() > 1e-9,
            _ => (left - right).abs() <= 1e-9,
        })
    }

    pub(crate) fn value(&self, word: &Word) -> Result<f64, ExpressionError> {
        let text = &word.value;
        if text.is_empty() {
            return Err(ExpressionError(format!("Adresse {} ohne Wert", word.address)));
        }
        if text.contains('P') {
            let negative = text.starts_with('-');
            let number = self.resolve(text.trim_start_matches(['+', '-']))?;
            return Ok(if negative { -number } else { number });
        }
        parse_float(text)
    }

    fn int_value(&self, word: &Word) -> Result<i64, ExpressionError> {
        Ok(py_int(self.value(word)?))
    }

    // ------------------------------------------------------------------ words
    fn run_words(&mut self, words: &[Word], call: Option<SubprogramRef>) -> Run {
        let is_pattern = words.iter().any(|w| w.address == "G" && w.value == "89");
        if let (Some(reference), false) = (call, is_pattern) {
            let mut count = 1;
            for w in words.iter().filter(|w| w.address == "L") {
                count = self.int_value(w)?;
            }
            return self.call(reference, count);
        }
        if let [only] = words {
            if only.address == "P" || only.address == "PP" {
                return self.execute_parameter(&format!("{}{}", only.address, only.value));
            }
        }
        let numbers: Vec<&Word> = words.iter().filter(|w| w.address == "N").collect();
        if let (Some(l_word), Some(first_n), Some(last_n)) =
            (words.iter().find(|w| w.address == "L"), numbers.first(), numbers.last())
        {
            let count = self.int_value(l_word)?;
            let first = self.int_value(first_n)?;
            if count == 0 {
                return Err(Flow::Jump(first));
            }
            let last = self.int_value(last_n)?;
            return self.repeat(first, last, count);
        }
        let g_words: Vec<&Word> = words.iter().filter(|w| w.address == "G").collect();
        let mut g_numbers: Vec<i64> = Vec::new();
        for w in g_words.iter().filter(|w| !w.value.is_empty()) {
            g_numbers.push(py_int(parse_float(&w.value)?));
        }
        let mut cycle: Option<&Word> = None;
        for w in g_words.iter().filter(|w| !w.value.is_empty()) {
            if matches!(py_int(parse_float(&w.value)?), 81..=87 | 89) {
                cycle = Some(w);
                break;
            }
        }
        self.apply_modal(words, &g_words)?;
        if let Some(cycle) = cycle {
            let number = py_int(parse_float(&cycle.value)?);
            let by: ByAddress<'_> =
                words.iter().filter(|w| w.address != "G").map(|w| (w.address.as_str(), w)).collect();
            cycles::run_cycle(self, number, cycle.variant, &by, words, call)?;
            return self.apply_m_after(words);
        }
        let has = |g: i64| g_numbers.contains(&g);
        if [53, 54, 55, 56, 58, 59].iter().any(|g| has(*g)) || g_words.iter().any(|w| w.value == "51") {
            self.apply_coordinate_system(words, &g_words)?;
            return self.apply_m_after(words);
        }
        if has(4) {
            let seconds = match words.iter().find(|w| w.address == "F") {
                Some(f) => self.value(f)?,
                None => 0.0,
            };
            self.dwell(seconds);
        }
        self.motion(words, &g_numbers)?;
        self.apply_m_after(words)
    }

    fn execute_parameter(&mut self, name: &str) -> Run {
        let content = match self.lookup(name)? {
            Memory::Text(text) => text,
            Memory::Number(_) => {
                return Err(Flow::Error(format!("{name} enthält einen Zahlenwert und keinen Satzinhalt")));
            }
        };
        self.call_depth += 1;
        if self.call_depth > self.config.max_call_depth {
            return Err(Flow::Stop("Abbruch: Parameter-Aufrufe zu tief verschachtelt".into()));
        }
        let result = self.run_text_statements(&content);
        self.call_depth -= 1;
        result
    }

    /// Run a word chain or subprogram reference (used by point patterns).
    pub(crate) fn execute_text(&mut self, text: &str) -> Run {
        let text = text.trim();
        if parameter_name_re().is_some_and(|re| re.is_match(text)) {
            return self.execute_parameter(text);
        }
        self.run_text_statements(text)
    }

    fn call(&mut self, reference: SubprogramRef, count: i64) -> Run {
        let Some(program) = self.programs.iter().find(|p| p.reference == Some(reference)).cloned() else {
            let text = format!("{} ist in der Datei nicht enthalten - Aufruf übersprungen", reference.label());
            self.msg(Severity::Error, &text);
            return Ok(());
        };
        self.call_depth += 1;
        if self.call_depth > self.config.max_call_depth {
            return Err(Flow::Stop("Abbruch: Unterprogramm-Aufrufe zu tief verschachtelt".into()));
        }
        let mut result = Ok(());
        for _ in 0..count.max(0) {
            result = self.run_program(Rc::clone(&program));
            if result.is_err() {
                break;
            }
        }
        self.call_depth -= 1;
        result
    }

    fn repeat(&mut self, first: i64, last: i64, count: i64) -> Run {
        let program = Rc::clone(&self.ctx.program);
        let range = match (program.index_of(first), program.index_of(last)) {
            (Some(start), Some(end)) if end >= start => Some((start, end)),
            _ => None,
        };
        let Some((start, end)) = range else {
            let text = format!("Programmteil N{first} bis N{last} nicht gefunden");
            self.msg(Severity::Error, &text);
            return Ok(());
        };
        self.call_depth += 1;
        if self.call_depth > self.config.max_call_depth {
            return Err(Flow::Stop("Abbruch: Wiederholungen zu tief verschachtelt".into()));
        }
        let mut result = Ok(());
        for _ in 0..count.max(0) {
            result = self.run_range(start, end + 1);
            if result.is_err() {
                break;
            }
        }
        self.call_depth -= 1;
        result
    }

    // ------------------------------------------------------------------ modal functions
    fn apply_modal(&mut self, words: &[Word], g_words: &[&Word]) -> Run {
        for w in g_words.iter().filter(|w| !w.value.is_empty()) {
            let g = py_int(parse_float(&w.value)?);
            if (0..=3).contains(&g) {
                self.motion = g;
                if g == 0 {
                    self.rapid_variant = w.variant.unwrap_or(0);
                }
            } else if let Some(plane) = plane_for(g) {
                if self.comp.is_some() {
                    self.warn("Ebenenwechsel bei aktiver Bahnkorrektur");
                }
                self.plane = plane;
                if let Some(variant) = w.variant.filter(|v| *v != 0) {
                    self.note(&format!("G{g}.{variant}: Variante der Ebene wird wie G{g} dargestellt"));
                }
            } else if g == 90 {
                self.absolute = true;
            } else if g == 91 {
                self.absolute = false;
            } else if let Some(mode) = corner_mode(g, w.variant) {
                self.corner_mode = mode.into();
            } else if g == 61 || g == 62 {
                self.note("G61/G62 (Radius-/Fasenübergang an Ecken) wird in der Grafik nicht berechnet");
            } else if matches!(g, 9 | 40 | 41 | 42 | 45 | 46 | 47 | 4 | 51 | 53 | 54 | 55 | 56 | 58 | 59) || g >= 81 {
                continue;
            } else {
                self.note(&format!("G{g} wird im Simulator nicht ausgewertet"));
            }
        }
        for w in words {
            match w.address.as_str() {
                "T" | "TT" => {
                    let number = self.int_value(w)?;
                    self.select_tool(number, w.address == "TT");
                }
                "S" => self.speed = self.value(w)?,
                "M" => {
                    let m = self.int_value(w)?;
                    if matches!(m, 3..=5) {
                        self.spindle = format!("M{m}");
                    } else if matches!(m, 60..=62) {
                        self.feed_mode = format!("M{m}");
                    } else if !matches!(m, 2 | 30 | 17 | 6 | 79) {
                        self.other_m.push(m);
                        if known_m(m).is_none() {
                            self.note(&format!("M{m}: Maschinenfunktion - im Simulator ohne Wirkung"));
                        }
                    }
                }
                _ => {}
            }
        }
        if !g_words.iter().any(|w| w.value == "4" || w.value == "04") {
            for w in words.iter().filter(|w| w.address == "F") {
                self.feed = self.value(w)?;
            }
        }
        Ok(())
    }

    fn select_tool(&mut self, number: i64, general: bool) {
        self.tool_label = format!("{}{number}", if general { "TT" } else { "T" });
        if number == 0 {
            self.tool = ToolData::with_radius(0.0);
            return;
        }
        match self.config.tools.get(&number) {
            Some(tool) => self.tool = tool.clone(),
            None => {
                self.tool = ToolData::with_radius(self.config.default_tool_radius);
                let text = format!(
                    "{}: nicht in der Simulator-Werkzeugtabelle - Radius {} mm angenommen",
                    self.tool_label,
                    py_g(self.config.default_tool_radius)
                );
                self.note(&text);
            }
        }
    }

    fn apply_m_after(&mut self, words: &[Word]) -> Run {
        for w in words.iter().filter(|w| w.address == "M") {
            let m = self.int_value(w)?;
            if m == 2 || m == 30 {
                if self.comp.is_some() {
                    self.flush_compensation(None, None)?;
                }
                self.record_state();
                return Err(Flow::Stop(format!("M{m} {}", known_m(m).unwrap_or(""))));
            }
            if m == 17 {
                if Rc::ptr_eq(&self.ctx.program, &self.main) {
                    self.warn("M17 im Hauptprogramm");
                } else {
                    return Err(Flow::Return);
                }
            }
            if m == 79 {
                match self.contour_pocket {
                    None => self.warn("M79 ohne aktive Konturtasche"),
                    Some((_depth, retract)) => {
                        if self.comp.is_some() {
                            self.flush_compensation(None, None)?;
                        }
                        self.contour_pocket = None;
                        let [u, v, _] = self.plane.to_plane(self.position());
                        let target = self.plane.from_plane(u, v, retract);
                        self.rapid_to(target, "Ende Konturtasche")?;
                    }
                }
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------------ coordinate systems
    fn apply_coordinate_system(&mut self, words: &[Word], g_words: &[&Word]) -> Run {
        let mut coords: [Option<f64>; 3] = [None; 3];
        let mut rotation: Option<f64> = None;
        for w in words {
            let base = if w.address == "A" || w.address == "I" {
                w.address.as_str()
            } else {
                w.address.trim_end_matches(['A', 'I'])
            };
            if let Some(index) = COORD_AXES.iter().position(|a| *a == base) {
                let value = self.value(w)?;
                if let Some(slot) = coords.get_mut(index) {
                    *slot = Some(value);
                }
            } else if matches!(w.address.as_str(), "W" | "WA" | "WI") {
                rotation = Some(self.value(w)?);
            }
        }
        let coord = |i: usize, default: f64| coords.get(i).copied().flatten().unwrap_or(default);
        for w in g_words {
            let g = py_int(parse_float(&w.value)?);
            match g {
                51 => {
                    let number = w.variant.unwrap_or(0);
                    let known = self.config.zero_points.get(&number).copied();
                    self.base = known.unwrap_or([0.0; 3]);
                    if known.is_none() && number != 0 {
                        self.note(&format!(
                            "G51.{number}: kein Eintrag in der Simulator-Nullpunkttabelle - 0/0/0 angenommen"
                        ));
                    }
                    self.base_label = format!("G51.{number}");
                }
                53 => {
                    self.shift = [0.0; 3];
                    self.rotation = 0.0;
                }
                54 => {
                    let current = self.position();
                    let mut shift = self.shift;
                    for (index, value) in coords.iter().enumerate() {
                        if let Some(value) = value {
                            let delta = (axis(&current, index) - value) * axis(&self.scale, index);
                            let updated = axis(&shift, index) + delta;
                            set_axis(&mut shift, index, updated);
                        }
                    }
                    self.shift = shift;
                }
                55 => {
                    let plain = Transform {
                        offset: [
                            self.base[0] + self.shift[0],
                            self.base[1] + self.shift[1],
                            self.base[2] + self.shift[2],
                        ],
                        rotation_deg: self.rotation,
                        plane: self.plane,
                        ..Transform::default()
                    };
                    let target = plain.apply([coord(0, 0.0), coord(1, 0.0), coord(2, 0.0)]);
                    self.shift = [target[0] - self.base[0], target[1] - self.base[1], target[2] - self.base[2]];
                    if let Some(rotation) = rotation {
                        self.rotation += rotation;
                    }
                }
                56 => {
                    self.shift = [coord(0, 0.0), coord(1, 0.0), coord(2, 0.0)];
                    self.rotation = rotation.unwrap_or(0.0);
                }
                58 => self.mirror = coords,
                59 => {
                    self.scale = [coord(0, 1.0), coord(1, 1.0), coord(2, 1.0)];
                    if self.scale.iter().any(|s| s.abs() < 1e-9) {
                        self.warn("G59 mit Maßstabsfaktor 0");
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------------ motion
    /// Cartesian target in program coordinates; second value: any axis programmed.
    fn target(&mut self, words: &[Word], start: Vec3) -> Run<(Vec3, bool)> {
        let mut target = start;
        let mut programmed = false;
        for w in words {
            for (index, name) in COORD_AXES.iter().enumerate() {
                let address = w.address.as_str();
                let plain = address == *name;
                if plain || address.strip_prefix(name).is_some_and(|rest| rest == "A" || rest == "I") {
                    let value = self.value(w)?;
                    let incremental = address.ends_with('I') || (plain && !self.absolute);
                    set_axis(&mut target, index, if incremental { axis(&start, index) + value } else { value });
                    programmed = true;
                }
            }
            if matches!(w.address.as_str(), "C" | "CA" | "CI") {
                let value = self.value(w)?;
                let incremental = w.address == "CI" || (w.address == "C" && !self.absolute);
                self.c_axis = if incremental { self.c_axis + value } else { value };
                self.note("C-Achse (Rundtisch): Stellung wird angezeigt, die Bahn aber ohne Drehung dargestellt");
            }
        }
        Ok((target, programmed))
    }

    fn motion(&mut self, words: &[Word], g_numbers: &[i64]) -> Run {
        let by = by_address(words);
        let has = |g: i64| g_numbers.contains(&g);
        let start = self.position();
        let (mut target, mut programmed) = self.target(words, start)?;
        let approach = [45, 46, 47].into_iter().find(|g| has(*g));
        let approach_r = match (approach, by.get("R")) {
            (Some(_), Some(r)) => self.value(r)?,
            _ => 0.0,
        };
        let polar =
            ["PW", "PWA", "PWI", "PL", "PX", "PY", "PXA", "PYA", "PXI", "PYI"].iter().any(|a| by.contains_key(a));
        let g9 = has(9);
        for address in ["UW", "UR", "FA"] {
            if by.contains_key(address) {
                self.note("Konturzug-Angaben (UW, UR, FA) werden im Simulator nicht berechnet");
            }
        }
        if polar && (self.motion == 0 || self.motion == 1) {
            target = self.polar_target(&by, start, target)?;
            programmed = true;
        }
        if g9 && (self.motion == 0 || self.motion == 1) {
            target = self.g9_line_target(&by, start, target)?;
            programmed = true;
        }
        if has(40) {
            self.comp_mode = 40;
            if self.comp.is_some() {
                self.flush_compensation(approach, Some(approach_r))?;
            }
            if programmed {
                let kind = if self.motion == 0 { MoveKind::Rapid } else { MoveKind::Feed };
                self.straight(target, kind, "")?;
            }
            return Ok(());
        }
        if has(41) || has(42) {
            let left = has(41);
            if self.comp.is_some() {
                self.flush_compensation(None, None)?;
            }
            self.comp_mode = if left { 41 } else { 42 };
            if self.tool.radius <= 0.0 {
                self.warn("Bahnkorrektur ohne Werkzeugradius (kein Werkzeug aufgerufen?)");
            }
            let entry_kind = if self.motion == 0 { MoveKind::Rapid } else { MoveKind::Feed };
            let contour_start = if programmed { self.transform().apply(target) } else { self.contour_w };
            let entry_feed =
                if entry_kind == MoveKind::Rapid { self.config.rapid_feed } else { self.effective_feed(None) };
            let from = self.contour_w;
            self.add_move(Track::Programmed, entry_kind, vec![from, contour_start], entry_feed, "", false, None, None)?;
            self.comp = Some(Compensation {
                left: left != self.transform().reverses_direction(),
                approach,
                approach_r,
                tool_start: self.tool_w,
                entry_kind,
                entry_line: self.line,
                entry_program: self.ctx.label.clone(),
                contour_start,
                elements: Vec::new(),
            });
            self.contour_w = contour_start;
            return Ok(());
        }
        if self.motion == 2 || self.motion == 3 {
            let has_center = ["I", "J", "K", "IA", "JA", "KA", "II", "JI", "KI"].iter().any(|a| by.contains_key(a));
            if programmed || has_center || g9 {
                self.arc(&by, start, target, programmed, g9)?;
            }
            return Ok(());
        }
        if !programmed {
            return Ok(());
        }
        if self.motion == 0 {
            self.rapid_to(target, "")
        } else {
            self.straight(target, MoveKind::Feed, "")
        }
    }

    fn first_of<'a>(by: &ByAddress<'a>, names: &[&str]) -> Option<&'a Word> {
        names.iter().find_map(|n| by.get(n).copied())
    }

    fn polar_target(&self, by: &ByAddress<'_>, start: Vec3, target: Vec3) -> Result<Vec3, ExpressionError> {
        let [pu, pv, _] = self.plane.to_plane(start);
        let mut pole = start;
        for (index, name) in POLAR_POLE.iter().enumerate() {
            for suffix in SUFFIXES {
                if let Some(w) = by.get(format!("{name}{suffix}").as_str()) {
                    let incremental = suffix == "I" || (suffix.is_empty() && !self.absolute);
                    let value = self.value(w)?;
                    set_axis(&mut pole, index, if incremental { axis(&start, index) + value } else { value });
                }
            }
        }
        let [cu, cv, _] = self.plane.to_plane(pole);
        let angle = match Self::first_of(by, &["PW", "PWA", "PWI"]) {
            Some(w) => self.value(w)?,
            None => (pv - cv).atan2(pu - cu).to_degrees(),
        };
        let length = match by.get("PL") {
            Some(w) => self.value(w)?,
            None => (pu - cu).hypot(pv - cv),
        };
        let [_, _, tw] = self.plane.to_plane(target);
        let a = angle.to_radians();
        Ok(self.plane.from_plane(cu + length * a.cos(), cv + length * a.sin(), tw))
    }

    fn center(&self, by: &ByAddress<'_>, start: Vec3) -> Result<Option<Vec2>, ExpressionError> {
        let mut center = start;
        let mut found = false;
        for (index, name) in CENTER_AXES.iter().enumerate() {
            for suffix in SUFFIXES {
                let Some(w) = by.get(format!("{name}{suffix}").as_str()) else { continue };
                let incremental = suffix == "I" || (suffix.is_empty() && !self.absolute);
                let value = self.value(w)?;
                set_axis(&mut center, index, if incremental { axis(&start, index) + value } else { value });
                if index == self.plane.main || index == self.plane.second {
                    found = true;
                }
            }
        }
        if !found {
            return Ok(None);
        }
        let [u, v, _] = self.plane.to_plane(center);
        Ok(Some((u, v)))
    }

    fn g9_line_target(&self, by: &ByAddress<'_>, start: Vec3, target: Vec3) -> Result<Vec3, ExpressionError> {
        let center = self.center(by, start)?;
        let [su, sv, _] = self.plane.to_plane(start);
        let (cu, cv) = center.unwrap_or((su, sv));
        let angle = match Self::first_of(by, &["W", "WA", "WI"]) {
            Some(w) => self.value(w)?,
            None => 0.0,
        };
        let length = match by.get("R") {
            Some(w) => self.value(w)?,
            None => (su - cu).hypot(sv - cv),
        };
        let [_, _, tw] = self.plane.to_plane(target);
        let a = angle.to_radians();
        Ok(self.plane.from_plane(cu + length * a.cos(), cv + length * a.sin(), tw))
    }

    fn arc(&mut self, by: &ByAddress<'_>, start: Vec3, target: Vec3, programmed: bool, g9: bool) -> Run {
        let clockwise = self.motion == 2;
        let mut center = self.center(by, start)?;
        let mut target = target;
        let [su, sv, _] = self.plane.to_plane(start);
        let mut sweep: Option<f64> = None;
        let mut full = false;
        if g9 {
            let Some((cu, cv)) = center else {
                self.msg(Severity::Error, "G9 im Kreissatz ohne Mittelpunkt (I, J, K)");
                return Ok(());
            };
            let radius = (su - cu).hypot(sv - cv);
            let [_, _, tw] = self.plane.to_plane(target);
            let angle = if let Some(wi) = by.get("WI") {
                let value = self.value(wi)?.abs() * if clockwise { -1.0 } else { 1.0 };
                sweep = Some(value);
                (sv - cv).atan2(su - cu).to_degrees() + value
            } else {
                match Self::first_of(by, &["W", "WA"]) {
                    Some(w) => self.value(w)?,
                    None => (sv - cv).atan2(su - cu).to_degrees(),
                }
            };
            let a = angle.to_radians();
            target = self.plane.from_plane(cu + radius * a.cos(), cv + radius * a.sin(), tw);
        } else if let Some((cu, cv)) = center {
            let [eu, ev, _] = self.plane.to_plane(target);
            if !programmed || ((eu - su).abs() < 1e-9 && (ev - sv).abs() < 1e-9) {
                full = true;
            } else {
                let r1 = (su - cu).hypot(sv - cv);
                let r2 = (eu - cu).hypot(ev - cv);
                if (r1 - r2).abs() > 0.01 {
                    self.warn(&format!("Kreis: Start- und Zielradius weichen ab ({r1:.3} / {r2:.3} mm)"));
                }
            }
        } else {
            let radius_word = by.get("R").copied();
            let Some(radius_word) = radius_word.filter(|_| programmed) else {
                self.msg(Severity::Error, "Kreissatz ohne Mittelpunkt (I, J, K) bzw. Radius");
                return Ok(());
            };
            let [eu, ev, _] = self.plane.to_plane(target);
            let radius = self.value(radius_word)?;
            let Some(found) = circle_center_from_radius((su, sv), (eu, ev), radius, clockwise) else {
                self.msg(Severity::Error, "Kreis mit Radius R: Zielpunkt mit diesem Radius nicht erreichbar");
                return Ok(());
            };
            self.note("Kreis über Radius R (Konturrechner): positives R = Bogen bis 180° (Annahme)");
            center = Some(found);
        }
        let Some(center) = center else { return Ok(()) };
        let mut points = arc_points(&self.plane, start, target, center, clockwise, full, sweep);
        if sweep.is_some() {
            if let Some(last) = points.last_mut() {
                *last = target;
            }
        }
        self.emit_program_points(points, MoveKind::Feed, "", None)
    }

    // ------------------------------------------------------------------ emitting moves
    /// Rapid move according to the active G0 variant (program coordinates).
    pub(crate) fn rapid_to(&mut self, target: Vec3, note: &str) -> Run {
        let start = self.position();
        let points = if self.rapid_variant == 2 || self.comp.is_some() {
            vec![target]
        } else if self.rapid_variant == 1 {
            let [su, sv, sw] = self.plane.to_plane(start);
            let [tu, tv, tw] = self.plane.to_plane(target);
            if tw >= sw {
                vec![self.plane.from_plane(su, sv, tw), target]
            } else {
                vec![self.plane.from_plane(tu, tv, sw), target]
            }
        } else {
            dog_leg(start, target)
        };
        self.emit_program_points(points, MoveKind::Rapid, note, None)
    }

    pub(crate) fn straight(&mut self, target: Vec3, kind: MoveKind, note: &str) -> Run {
        match kind {
            MoveKind::Rapid => self.rapid_to(target, note),
            MoveKind::Feed => self.emit_program_points(vec![target], kind, note, None),
        }
    }

    pub(crate) fn feed_to(&mut self, target: Vec3, note: &str, feed: Option<f64>) -> Run {
        self.emit_program_points(vec![target], MoveKind::Feed, note, feed)
    }

    pub(crate) fn arc_to(
        &mut self,
        target: Vec3,
        center_uv: Vec2,
        clockwise: bool,
        note: &str,
        full_circle: bool,
        feed: Option<f64>,
    ) -> Run {
        let points = arc_points(&self.plane, self.position(), target, center_uv, clockwise, full_circle, None);
        self.emit_program_points(points, MoveKind::Feed, note, feed)
    }

    pub(crate) fn dwell(&mut self, seconds: f64) {
        self.result.dwells.push(Dwell {
            seconds,
            line: self.line,
            state: self.result.snapshots.len(),
            after_move: self.result.moves.len(),
        });
    }

    fn emit_program_points(&mut self, points: Vec<Vec3>, kind: MoveKind, note: &str, feed: Option<f64>) -> Run {
        let points = match (self.contour_pocket, kind) {
            (Some((level, _)), MoveKind::Feed) => points
                .into_iter()
                .map(|p| {
                    let [u, v, _] = self.plane.to_plane(p);
                    self.plane.from_plane(u, v, level)
                })
                .collect(),
            _ => points,
        };
        let transform = self.transform();
        let w_points: Vec<Vec3> = points.into_iter().map(|p| transform.apply(p)).collect();
        let Some(last) = w_points.last().copied() else { return Ok(()) };
        let use_feed = match kind {
            MoveKind::Feed => self.effective_feed(feed),
            MoveKind::Rapid => self.config.rapid_feed,
        };
        let programmed: Vec<Vec3> = std::iter::once(self.contour_w).chain(w_points.iter().copied()).collect();
        if let Some(comp) = self.comp.as_mut() {
            comp.elements.push(Element {
                kind,
                points: w_points,
                line: self.line,
                program: self.ctx.label.clone(),
                feed: use_feed,
            });
            self.add_move(Track::Programmed, kind, programmed, use_feed, note, false, None, None)?;
            self.contour_w = last;
            return Ok(());
        }
        self.add_move(Track::Programmed, kind, programmed, use_feed, note, false, None, None)?;
        let tool: Vec<Vec3> = std::iter::once(self.tool_w).chain(w_points).collect();
        self.add_move(Track::Moves, kind, tool, use_feed, note, false, None, None)?;
        self.contour_w = last;
        self.tool_w = last;
        Ok(())
    }

    fn effective_feed(&mut self, feed: Option<f64>) -> f64 {
        let value = feed.unwrap_or(self.feed);
        if value <= 0.0 {
            if !self.feed_warned {
                self.feed_warned = true;
                let text =
                    format!("Kein Vorschub F programmiert - {} mm/min angenommen", py_g(self.config.default_feed));
                self.warn(&text);
            }
            return self.config.default_feed;
        }
        value
    }

    #[allow(clippy::too_many_arguments)]
    fn add_move(
        &mut self,
        track: Track,
        kind: MoveKind,
        points: Vec<Vec3>,
        feed: f64,
        note: &str,
        compensated: bool,
        line: Option<usize>,
        program: Option<&str>,
    ) -> Run {
        let length: f64 = points.iter().zip(points.iter().skip(1)).map(|(a, b)| distance(*a, *b)).sum();
        if length < 1e-9 {
            return Ok(());
        }
        self.points_total += points.len();
        if self.points_total > self.config.max_points {
            return Err(Flow::Stop("Abbruch: zu viele Bahnpunkte".into()));
        }
        let duration = if feed > 0.0 { length / feed * 60.0 } else { 0.0 };
        let entry = Move {
            kind,
            points,
            line: line.unwrap_or(self.line),
            program: program.map_or_else(|| self.ctx.label.clone(), ToString::to_string),
            state: self.result.snapshots.len(),
            feed,
            length,
            duration,
            note: note.to_string(),
            compensated,
        };
        match track {
            Track::Moves => self.result.moves.push(entry),
            Track::Programmed => self.result.programmed.push(entry),
        }
        Ok(())
    }

    // ------------------------------------------------------------------ radius compensation
    fn flush_compensation(&mut self, approach: Option<i64>, approach_r: Option<f64>) -> Run {
        let comp = self.comp.take();
        self.comp_mode = 40;
        let Some(comp) = comp else { return Ok(()) };
        let first_point = comp.contour_start;
        let mut points: Vec<Vec3> = vec![first_point];
        let mut sources: Vec<usize> = vec![0];
        for (index, element) in comp.elements.iter().enumerate() {
            points.extend(element.points.iter().copied());
            sources.extend(std::iter::repeat_n(index + 1, element.points.len()));
        }
        let radius = self.tool.radius;
        let (Some(p0), Some(p1)) = (points.first().copied(), points.get(1).copied()) else {
            self.warn("Bahnkorrektur ohne Konturelemente");
            let from = self.tool_w;
            let feed = self.config.rapid_feed;
            self.add_move(Track::Moves, comp.entry_kind, vec![from, first_point], feed, "", false, None, None)?;
            self.tool_w = first_point;
            return Ok(());
        };
        let plane = self.plane;
        let (path, path_src) = offset_polyline(&points, &sources, &plane, radius, comp.left);
        let (Some(path0), Some(src0)) = (path.first().copied(), path_src.first().copied()) else { return Ok(()) };
        // approach
        let d0 = direction(&plane, p0, p1);
        let approach_pts = approach_points(&plane, path0, d0, comp.left, comp.approach, comp.approach_r);
        let entry_feed =
            if comp.entry_kind == MoveKind::Rapid { self.config.rapid_feed } else { self.effective_feed(None) };
        if comp.approach.is_some() {
            self.note("An- und Wegfahren (G45/G46/G47) vereinfacht dargestellt");
        }
        let entry_end = approach_pts.first().copied().unwrap_or(path0);
        self.add_move(
            Track::Moves,
            comp.entry_kind,
            vec![comp.tool_start, entry_end],
            entry_feed,
            "Anfahren",
            true,
            Some(comp.entry_line),
            Some(&comp.entry_program),
        )?;
        if !approach_pts.is_empty() {
            let mut pts = approach_pts;
            pts.push(path0);
            let feed = self.effective_feed(None);
            let note = format!("Anfahren G{}", comp.approach.unwrap_or(0));
            self.add_move(
                Track::Moves,
                MoveKind::Feed,
                pts,
                feed,
                &note,
                true,
                Some(comp.entry_line),
                Some(&comp.entry_program),
            )?;
        }
        // contour elements
        let mut group: Vec<Vec3> = vec![path0];
        let mut group_src = if src0 > 0 { src0 } else { 1 };
        for (p, raw_src) in path.iter().zip(path_src.iter()).skip(1) {
            let s = (*raw_src).max(1);
            if s != group_src && group.len() > 1 {
                let last = group.last().copied();
                self.emit_comp_group(&comp, group_src, std::mem::take(&mut group))?;
                group.extend(last);
            }
            group_src = s;
            group.push(*p);
        }
        if group.len() > 1 {
            self.emit_comp_group(&comp, group_src, group)?;
        }
        let mut end = path.last().copied().unwrap_or(path0);
        // departure
        if let (Some(kind), Some(r)) = (approach, approach_r.filter(|r| *r != 0.0)) {
            let n = points.len();
            let (Some(a), Some(b)) = (points.get(n.wrapping_sub(2)).copied(), points.last().copied()) else {
                return Ok(());
            };
            let d_last = direction(&plane, a, b);
            let leave = departure_points(&plane, end, d_last, comp.left, kind, r);
            let feed = self.effective_feed(None);
            let note = format!("Wegfahren G{kind}");
            let last_leave = leave.last().copied();
            let pts: Vec<Vec3> = std::iter::once(end).chain(leave).collect();
            self.add_move(Track::Moves, MoveKind::Feed, pts, feed, &note, true, None, None)?;
            if let Some(last) = last_leave {
                end = last;
            }
        }
        self.tool_w = end;
        self.contour_w = end;
        Ok(())
    }

    fn emit_comp_group(&mut self, comp: &Compensation, source: usize, points: Vec<Vec3>) -> Run {
        let Some(element) = comp.elements.get(source.wrapping_sub(1)) else { return Ok(()) };
        let (kind, feed, line, program) = (element.kind, element.feed, element.line, element.program.clone());
        self.add_move(Track::Moves, kind, points, feed, "", true, Some(line), Some(&program))
    }
}

/// Rapid without interpolation: all axes move at the same speed and stop independently.
fn dog_leg(start: Vec3, target: Vec3) -> Vec<Vec3> {
    let deltas = [target[0] - start[0], target[1] - start[1], target[2] - start[2]];
    let mut distances: Vec<f64> = deltas.iter().map(|d| d.abs()).filter(|d| *d > 1e-9).collect();
    distances.sort_by(f64::total_cmp);
    distances.dedup_by(|a, b| a.total_cmp(b).is_eq());
    let mut points: Vec<Vec3> = distances
        .iter()
        .map(|travelled| {
            let mut p = start;
            for ((value, delta), origin) in p.iter_mut().zip(deltas.iter()).zip(start.iter()) {
                *value = origin + delta.abs().min(*travelled).copysign(*delta);
            }
            p
        })
        .collect();
    match points.last_mut() {
        Some(last) => *last = target,
        None => return vec![target],
    }
    points
}

/// Start point(s) of the approach to `first` (tool centre on the contour start).
fn approach_points(plane: &Plane, first: Vec3, d0: Vec2, left: bool, kind: Option<i64>, r: f64) -> Vec<Vec3> {
    let Some(kind) = kind else { return Vec::new() };
    if r <= 0.0 {
        return Vec::new();
    }
    let [u, v, w] = plane.to_plane(first);
    let side = if left { 1.0 } else { -1.0 };
    let n = (-d0.1 * side, d0.0 * side); // towards the tool side
    if kind == 45 {
        return vec![plane.from_plane(u - d0.0 * r, v - d0.1 * r, w)];
    }
    let (cu, cv) = (u + n.0 * r, v + n.1 * r);
    let start = if kind == 47 {
        plane.from_plane(cu - d0.0 * r, cv - d0.1 * r, w)
    } else {
        plane.from_plane(u + 2.0 * n.0 * r, v + 2.0 * n.1 * r, w)
    };
    let mut arc = arc_points(plane, start, first, (cu, cv), !left, false, None);
    arc.pop();
    std::iter::once(start).chain(arc).collect()
}

fn departure_points(plane: &Plane, last: Vec3, d: Vec2, left: bool, kind: i64, r: f64) -> Vec<Vec3> {
    let [u, v, w] = plane.to_plane(last);
    let side = if left { 1.0 } else { -1.0 };
    let n = (-d.1 * side, d.0 * side);
    if kind == 45 {
        return vec![plane.from_plane(u + d.0 * r, v + d.1 * r, w)];
    }
    let (cu, cv) = (u + n.0 * r, v + n.1 * r);
    let end = if kind == 47 {
        plane.from_plane(cu + d.0 * r, cv + d.1 * r, w)
    } else {
        plane.from_plane(u + 2.0 * n.0 * r, v + 2.0 * n.1 * r, w)
    };
    arc_points(plane, last, end, (cu, cv), !left, false, None)
}

/// Simulate `text` (program text of one file).
pub fn simulate(text: &str, config: &SimConfig) -> SimResult {
    Interpreter::new(text, config.clone()).run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::reference::DEMO_PROGRAM;

    #[test]
    fn demo_program_runs_to_m30() {
        let result = simulate(DEMO_PROGRAM, &SimConfig::default());
        assert_eq!(result.end_reason, "M30 Programmende mit Rücksprung");
        assert!(result.errors().is_empty(), "{:?}", result.errors());
        assert!(result.moves.len() > 50);
        assert_eq!(result.parameters, vec![("P1".to_string(), "5".to_string()), ("P2".into(), "%*1".into())]);
    }

    #[test]
    fn jumps_loops_and_errors() {
        let text = "N10 P1 = 0\nN20 P1 = (P1 + 1)\nN30 IF P1 < 3 THEN L0 N20\nN40 G1 X10 F100; Q?\nN50 L0 N99\n";
        let result = simulate(text, &SimConfig::default());
        assert_eq!(result.end_reason, "Sprungziel N99 nicht gefunden");
        assert_eq!(result.parameters, vec![("P1".to_string(), "3".to_string())]);
        let text = "N10 P1 = (P1)\nN20 G1 GP1\nM30\n";
        let result = simulate(text, &SimConfig::default());
        let errors: Vec<&str> = result.errors().iter().map(|m| m.text.as_str()).collect();
        assert_eq!(errors, ["Parameter P1 ist nicht belegt", "could not convert string to float: 'P1'"]);
        let mut config = SimConfig::default();
        config.parameter_table.insert("P5".into(), "(P5 + 1)".into());
        let result = simulate("N10 X P5\nM30\n", &config);
        assert_eq!(result.errors().len(), 1);
    }
}
