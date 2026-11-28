//! Result data of a simulation run.

use std::collections::BTreeMap;

use crate::sim::geometry::Vec3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveKind {
    Rapid,
    Feed,
}

impl MoveKind {
    pub fn value(self) -> &'static str {
        match self {
            Self::Rapid => "rapid",
            Self::Feed => "feed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    pub fn value(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Info => "Hinweis",
            Self::Warning => "Warnung",
            Self::Error => "Fehler",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolData {
    pub radius: f64,
    pub length: f64,
    pub name: String,
}

impl ToolData {
    pub fn with_radius(radius: f64) -> Self {
        Self { radius, length: 0.0, name: String::new() }
    }
}

impl Default for ToolData {
    fn default() -> Self {
        Self::with_radius(5.0)
    }
}

/// Control state after a block (shown in the position and modal displays).
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub line: usize,
    pub program: String,
    /// Programmed position in the active program coordinate system.
    pub position: Vec3,
    pub modal: Vec<(String, String)>,
    pub tool: String,
    pub tool_radius: f64,
    pub feed: f64,
    pub speed: f64,
    pub spindle: String,
    pub c_axis: f64,
}

impl Snapshot {
    pub fn empty() -> Self {
        Self {
            line: 0,
            program: String::new(),
            position: [0.0; 3],
            modal: Vec::new(),
            tool: "T0".into(),
            tool_radius: 0.0,
            feed: 0.0,
            speed: 0.0,
            spindle: "M5".into(),
            c_axis: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Move {
    pub kind: MoveKind,
    /// Workpiece coordinates, first point = start (never empty).
    pub points: Vec<Vec3>,
    pub line: usize,
    pub program: String,
    /// Index into [`SimResult::snapshots`].
    pub state: usize,
    /// mm/min used for the time estimate.
    pub feed: f64,
    pub length: f64,
    /// Seconds.
    pub duration: f64,
    pub note: String,
    pub compensated: bool,
}

impl Move {
    pub fn start(&self) -> Vec3 {
        self.points.first().copied().unwrap_or_default()
    }

    pub fn end(&self) -> Vec3 {
        self.points.last().copied().unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub severity: Severity,
    pub text: String,
    pub line: usize,
    pub program: String,
}

impl std::fmt::Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let place = if self.line > 0 { format!("Zeile {}", self.line) } else { "Programm".into() };
        write!(f, "{}: {place} ({}): {}", self.severity.label(), self.program, self.text)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Dwell {
    pub seconds: f64,
    pub line: usize,
    pub state: usize,
    /// Number of moves emitted before the dwell.
    pub after_move: usize,
}

/// Assumptions of the simulation (not machine data).
#[derive(Debug, Clone, PartialEq)]
pub struct SimConfig {
    /// mm/min - FP2A value unknown, simulation assumption.
    pub rapid_feed: f64,
    pub default_feed: f64,
    pub start_position: Vec3,
    pub tools: BTreeMap<i64, ToolData>,
    pub zero_points: BTreeMap<i64, Vec3>,
    /// "P1" -> "50" / "(P2+1)" / "G81 ...".
    pub parameter_table: BTreeMap<String, String>,
    pub default_tool_radius: f64,
    pub max_blocks: usize,
    pub max_points: usize,
    pub max_call_depth: usize,
}

impl Default for SimConfig {
    fn default() -> Self {
        Self {
            rapid_feed: 5000.0,
            default_feed: 100.0,
            start_position: [0.0, 0.0, 100.0],
            tools: BTreeMap::new(),
            zero_points: BTreeMap::new(),
            parameter_table: BTreeMap::new(),
            default_tool_radius: 5.0,
            max_blocks: 200_000,
            max_points: 2_000_000,
            max_call_depth: 32,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SimResult {
    pub moves: Vec<Move>,
    /// Contour without radius compensation.
    pub programmed: Vec<Move>,
    pub snapshots: Vec<Snapshot>,
    pub messages: Vec<Message>,
    pub dwells: Vec<Dwell>,
    pub executed_blocks: usize,
    pub end_reason: String,
    /// Final parameter memory in assignment order.
    pub parameters: Vec<(String, String)>,
    pub program_labels: Vec<String>,
}

impl SimResult {
    pub fn total_time_s(&self) -> f64 {
        self.moves.iter().map(|m| m.duration).sum::<f64>() + self.dwells.iter().map(|d| d.seconds).sum::<f64>()
    }

    pub fn feed_length(&self) -> f64 {
        self.moves.iter().filter(|m| m.kind == MoveKind::Feed).map(|m| m.length).sum()
    }

    pub fn rapid_length(&self) -> f64 {
        self.moves.iter().filter(|m| m.kind == MoveKind::Rapid).map(|m| m.length).sum()
    }

    pub fn errors(&self) -> Vec<&Message> {
        self.messages.iter().filter(|m| m.severity == Severity::Error).collect()
    }

    pub fn warnings(&self) -> Vec<&Message> {
        self.messages.iter().filter(|m| m.severity == Severity::Warning).collect()
    }

    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut points = self.moves.iter().flat_map(|m| m.points.iter());
        let first = *points.next()?;
        let (mut lo, mut hi) = (first, first);
        for p in points {
            for ((l, h), v) in lo.iter_mut().zip(hi.iter_mut()).zip(p.iter()) {
                *l = l.min(*v);
                *h = h.max(*v);
            }
        }
        Some((lo, hi))
    }
}
