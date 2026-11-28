//! Time-based playback of a simulation result (independent of the GUI).

use std::collections::BTreeMap;

use crate::sim::geometry::{distance, lerp, Vec3};
use crate::sim::model::{Move, SimResult, Snapshot};

/// Tool state at a point in time.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub time: f64,
    /// Workpiece coordinates.
    pub position: Vec3,
    /// Index of the current move (None before the first move).
    pub move_index: Option<usize>,
    /// 0..1 within the current move.
    pub fraction: f64,
    pub line: usize,
    pub snapshot: Snapshot,
    /// Distance to the end of the current move per axis.
    pub remaining: Vec3,
    pub finished: bool,
}

/// Maps simulation time to tool position, line and control state.
#[derive(Debug, Clone)]
pub struct Playback {
    pub result: SimResult,
    pub start_position: Vec3,
    pub starts: Vec<f64>,
    pub durations: Vec<f64>,
    pub total: f64,
    cumulative: Vec<Vec<f64>>,
}

fn cumulative(entry: &Move) -> Vec<f64> {
    let mut out = vec![0.0];
    let mut sum = 0.0;
    for (a, b) in entry.points.iter().zip(entry.points.iter().skip(1)) {
        sum += distance(*a, *b);
        out.push(sum);
    }
    out
}

impl Playback {
    pub fn new(result: SimResult, start_position: Vec3) -> Self {
        let mut dwell_after: BTreeMap<usize, f64> = BTreeMap::new();
        for dwell in &result.dwells {
            *dwell_after.entry(dwell.after_move).or_insert(0.0) += dwell.seconds;
        }
        let after = |index: usize| dwell_after.get(&index).copied().unwrap_or(0.0);
        let mut t = after(0);
        let mut starts = Vec::with_capacity(result.moves.len());
        let mut durations = Vec::with_capacity(result.moves.len());
        for (index, entry) in result.moves.iter().enumerate() {
            starts.push(t);
            let duration = entry.duration.max(1e-6);
            durations.push(duration);
            t += duration + after(index + 1);
        }
        let cumulative = result.moves.iter().map(cumulative).collect();
        Self { result, start_position, starts, durations, total: t, cumulative }
    }

    pub fn frame(&self, time: f64) -> Frame {
        let moves = &self.result.moves;
        let snapshots = &self.result.snapshots;
        let time = time.max(0.0).min(self.total);
        if moves.is_empty() {
            let snap = snapshots.last().cloned().unwrap_or_else(Snapshot::empty);
            return Frame {
                time,
                position: self.start_position,
                move_index: None,
                fraction: 0.0,
                line: snap.line,
                snapshot: snap,
                remaining: [0.0; 3],
                finished: true,
            };
        }
        let after = self.starts.partition_point(|s| *s <= time);
        let (Some(index), Some(entry)) = (after.checked_sub(1), after.checked_sub(1).and_then(|i| moves.get(i))) else {
            let snap = snapshots.first().cloned().unwrap_or_else(Snapshot::empty);
            return Frame {
                time,
                position: moves.first().map(Move::start).unwrap_or(self.start_position),
                move_index: None,
                fraction: 0.0,
                line: 0,
                snapshot: snap,
                remaining: [0.0; 3],
                finished: false,
            };
        };
        let start = self.starts.get(index).copied().unwrap_or(0.0);
        let duration = self.durations.get(index).copied().unwrap_or(1e-6);
        let fraction = ((time - start) / duration).min(1.0);
        let position = self.point_on(index, fraction);
        let snap_index = entry.state.min(snapshots.len().saturating_sub(1));
        let snapshot = snapshots.get(snap_index).cloned().unwrap_or_else(Snapshot::empty);
        let end = entry.end();
        Frame {
            time,
            position,
            move_index: Some(index),
            fraction,
            line: entry.line,
            snapshot,
            remaining: [end[0] - position[0], end[1] - position[1], end[2] - position[2]],
            finished: time >= self.total - 1e-9,
        }
    }

    pub fn point_on(&self, index: usize, fraction: f64) -> Vec3 {
        let (Some(entry), Some(cumulative)) = (self.result.moves.get(index), self.cumulative.get(index)) else {
            return self.start_position;
        };
        let target = cumulative.last().copied().unwrap_or(0.0) * fraction;
        let k = cumulative.partition_point(|c| *c < target);
        if k == 0 {
            return entry.start();
        }
        if k >= cumulative.len() {
            return entry.end();
        }
        let (Some(c_prev), Some(c_k), Some(p_prev), Some(p_k)) =
            (cumulative.get(k - 1), cumulative.get(k), entry.points.get(k - 1), entry.points.get(k))
        else {
            return entry.end();
        };
        let seg = c_k - c_prev;
        let t = if seg <= 0.0 { 0.0 } else { (target - c_prev) / seg };
        lerp(*p_prev, *p_k, t)
    }

    pub fn time_of_move_end(&self, index: usize) -> f64 {
        let Some(last) = self.starts.len().checked_sub(1) else { return 0.0 };
        let index = index.min(last);
        self.starts.get(index).copied().unwrap_or(0.0) + self.durations.get(index).copied().unwrap_or(0.0)
    }

    /// End time of the block that is active at `time` (single block mode).
    pub fn next_block_end(&self, time: f64) -> f64 {
        let moves = &self.result.moves;
        if moves.is_empty() {
            return self.total;
        }
        let mut index = self.starts.partition_point(|s| *s <= time).saturating_sub(1);
        if time >= self.time_of_move_end(index) - 1e-9 {
            index += 1;
        }
        let Some(current) = moves.get(index) else { return self.total };
        let key = (current.line, current.state);
        while moves.get(index + 1).is_some_and(|m| (m.line, m.state) == key) {
            index += 1;
        }
        self.time_of_move_end(index)
    }

    /// End of the last move before the program first moves past `line` (transfer follow-up).
    pub fn time_until_line(&self, line: usize) -> f64 {
        let mut reached = 0.0;
        for (index, entry) in self.result.moves.iter().enumerate() {
            if entry.line > line {
                break;
            }
            reached = self.time_of_move_end(index);
        }
        reached
    }

    pub fn time_of_line(&self, line: usize) -> Option<f64> {
        let index = self.result.moves.iter().position(|m| m.line == line)?;
        self.starts.get(index).copied()
    }
}

/// `H:MM:SS` with Python rounding (half to even).
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn format_duration(seconds: f64) -> String {
    let total = seconds.max(0.0).round_ties_even();
    let total = if total.is_finite() { total as u64 } else { 0 };
    let (hours, rest) = (total / 3600, total % 3600);
    let (minutes, secs) = (rest / 60, rest % 60);
    format!("{hours}:{minutes:02}:{secs:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::model::SimConfig;
    use crate::sim::reference::DEMO_PROGRAM;
    use crate::sim::simulate;

    #[test]
    fn playback_walks_the_demo() {
        let config = SimConfig::default();
        let playback = Playback::new(simulate(DEMO_PROGRAM, &config), config.start_position);
        assert!(playback.total > 10.0);
        let first = playback.frame(0.0);
        assert_eq!(first.move_index, Some(0));
        let last = playback.frame(playback.total + 5.0);
        assert!(last.finished);
        let end = playback.next_block_end(0.0);
        assert!(end > 0.0 && end <= playback.total);
        assert_eq!(format_duration(3725.5), "1:02:06");
        assert_eq!(format_duration(2.5), "0:00:02");
    }
}
