//! Machining cycles G81-G86, G87.x and G89.x (simplified tool paths).
//!
//! Addresses and sequences follow the training documentation (module 8). Where the
//! documentation leaves details open (step-over without AM, retract during chip
//! breaking, end position of pocket cycles ...) the simulator uses a documented
//! assumption and reports it as a note.

use crate::sim::geometry::{ceil_i64, Vec2, Vec3};
use crate::sim::interpreter::{ByAddress, Flow, Interpreter, Run};
use crate::sim::model::Severity;
use crate::sim::py_int;
use crate::sim::syntax::{SubprogramRef, Word};

pub fn drill_name(number: i64) -> Option<&'static str> {
    Some(match number {
        81 => "Bohren",
        82 => "Bohren mit Spanbrechen",
        83 => "Tieflochbohren",
        84 => "Gewindebohren",
        85 => "Reiben",
        86 => "Ausdrehen",
        _ => return None,
    })
}

pub fn mill_name(variant: i64) -> Option<&'static str> {
    Some(match variant {
        1 => "Rechtecktasche",
        2 => "Kreistasche",
        3 => "Zapfen",
        4 => "Außengewinde fräsen",
        5 => "Innengewinde fräsen",
        9 => "Konturtasche",
        _ => return None,
    })
}

pub fn pattern_name(variant: i64) -> Option<&'static str> {
    match variant {
        1 => Some("Punktemuster Gerade"),
        2 => Some("Punktemuster Teilkreis"),
        _ => None,
    }
}

pub const MAX_LEVELS: usize = 500;
pub const MAX_PATTERN_POINTS: usize = 5000;

fn cycle_error(text: impl Into<String>) -> Flow {
    Flow::Cycle(text.into())
}

/// Python's repr of an optional variant in messages (`G87.None`).
fn variant_text(variant: Option<i64>) -> String {
    variant.map_or_else(|| "None".to_string(), |v| v.to_string())
}

pub fn run_cycle(
    it: &mut Interpreter,
    number: i64,
    variant: Option<i64>,
    words: &ByAddress<'_>,
    ordered: &[Word],
    call: Option<SubprogramRef>,
) -> Run {
    let result = if drill_name(number).is_some() {
        drill(it, number, words, ordered)
    } else if number == 87 {
        match variant {
            Some(1) => rect_pocket(it, words, ordered),
            Some(2) => circle_pocket(it, words, ordered),
            Some(3) => boss(it, words, ordered),
            Some(v @ (4 | 5)) => thread(it, v, words, ordered),
            Some(9) => contour_pocket(it, words),
            _ => Err(cycle_error(format!("G87.{}: unbekannte Zyklusvariante", variant_text(variant)))),
        }
    } else if number == 89 {
        match variant {
            Some(v @ (1 | 2)) => pattern(it, v, words, ordered, call),
            _ => Err(cycle_error(format!("G89.{}: unbekannte Musterart", variant_text(variant)))),
        }
    } else {
        Err(cycle_error(format!("G{number}: kein Zyklus")))
    };
    match result {
        Err(Flow::Cycle(text)) => {
            it.msg(Severity::Error, &text);
            Ok(())
        }
        other => other,
    }
}

// ----------------------------------------------------------------------- helpers
fn get(it: &Interpreter, words: &ByAddress<'_>, names: &[&str]) -> Run<Option<f64>> {
    for name in names {
        if let Some(word) = words.get(name) {
            return Ok(Some(it.value(word)?));
        }
    }
    Ok(None)
}

/// Python `value or default` for an optional number.
fn or(value: Option<f64>, default: f64) -> f64 {
    match value {
        Some(v) if v != 0.0 => v,
        _ => default,
    }
}

/// (surface, depth, safety, retract) on the tool axis, program coordinates.
fn levels(it: &mut Interpreter, words: &ByAddress<'_>, w0: f64) -> Run<(f64, f64, f64, f64)> {
    let (oa, oi) = (get(it, words, &["OA"])?, get(it, words, &["OI"])?);
    let surface = match (oa, oi) {
        (Some(a), _) => a,
        (None, Some(i)) => w0 - i.abs(),
        (None, None) => w0,
    };
    if oa.is_none() && oi.is_none() {
        it.note("Zyklus ohne Oberfläche (OA/OI): Inkrementalangaben beziehen sich auf die Werkzeugposition");
    }
    let (ta, ti) = (get(it, words, &["TA"])?, get(it, words, &["TI"])?);
    let depth = match (ta, ti) {
        (Some(a), _) => a,
        (None, Some(i)) => surface - i.abs(),
        (None, None) => return Err(cycle_error("Zyklus ohne Tiefe (TA oder TI)")),
    };
    let (sa, si) = (get(it, words, &["SA"])?, get(it, words, &["SI"])?);
    let safety = match (sa, si) {
        (Some(a), _) => a,
        (None, Some(i)) => surface + i.abs(),
        (None, None) => surface,
    };
    let (ea, ei) = (get(it, words, &["EA"])?, get(it, words, &["EI"])?);
    let retract = match (ea, ei) {
        (Some(a), _) => a,
        (None, Some(i)) => surface + i.abs(),
        (None, None) => safety,
    };
    if ea.is_none() && ei.is_none() {
        it.warn("Zyklus ohne Rückzugsebene (EA/EI): Rückzug nur auf Sicherheitsabstand - Kollisionsgefahr");
    }
    if depth > surface {
        it.warn("Zyklus: Tiefe liegt oberhalb der Oberfläche");
    }
    Ok((surface, depth, safety, retract))
}

fn dwell_and_feed(it: &Interpreter, ordered: &[Word]) -> Run<(f64, Option<f64>)> {
    let mut dwell = 0.0;
    let mut feed = None;
    let mut previous: Option<&Word> = None;
    for word in ordered {
        if word.address == "F" {
            if previous.is_some_and(|p| p.address == "G" && (p.value == "4" || p.value == "04")) {
                dwell = it.value(word)?;
            } else {
                feed = Some(it.value(word)?);
            }
        }
        previous = Some(word);
    }
    Ok((dwell, feed))
}

fn point(it: &Interpreter, u: f64, v: f64, w: f64) -> Vec3 {
    it.plane.from_plane(u, v, w)
}

fn uvw(it: &Interpreter) -> Vec3 {
    it.plane.to_plane(it.position())
}

fn infeeds(it: &Interpreter, words: &ByAddress<'_>, start: f64, depth: f64) -> Run<Vec<f64>> {
    let mi = get(it, words, &["MI"])?;
    let di = or(get(it, words, &["DI"])?, 0.0);
    let total = start - depth;
    let Some(mi) = mi.filter(|m| *m > 0.0 && total > 0.0) else { return Ok(vec![depth]) };
    let mut levels = Vec::new();
    let mut step = mi.abs();
    let mut level = start;
    let minimum = di.abs().max(step * 0.1).max(0.01);
    while level - step > depth + 1e-9 && levels.len() < MAX_LEVELS {
        level -= step;
        levels.push(level);
        step = (step - di.abs()).max(minimum);
    }
    levels.push(depth);
    Ok(levels)
}

// ----------------------------------------------------------------------- drilling
fn drill(it: &mut Interpreter, number: i64, words: &ByAddress<'_>, ordered: &[Word]) -> Run {
    let name = format!("G{number} {}", drill_name(number).unwrap_or(""));
    let [u0, v0, w0] = uvw(it);
    let (surface, depth, safety, retract) = levels(it, words, w0)?;
    let (dwell, mut feed) = dwell_and_feed(it, ordered)?;
    if let Some(fz) = get(it, words, &["FZ"])? {
        feed = Some(fz);
    }
    it.rapid_to(point(it, u0, v0, safety), &name)?;
    if number == 82 || number == 83 {
        let steps = infeeds(it, words, surface, depth)?;
        if number == 82 {
            it.note("G82 Spanbrechen: kurzer Rückzug nach jeder Zustellung wird nicht dargestellt");
            for level in &steps {
                it.feed_to(point(it, u0, v0, *level), &name, feed)?;
            }
        } else {
            it.note("G83: Wiederanfahren im Eilgang bis 1 mm über den erreichten Bohrgrund (Annahme)");
            let mut reached = surface;
            let last = steps.last().copied();
            for level in &steps {
                if reached < surface {
                    it.rapid_to(point(it, u0, v0, reached + 1.0), &name)?;
                }
                it.feed_to(point(it, u0, v0, *level), &name, feed)?;
                reached = *level;
                #[allow(clippy::float_cmp)]
                if Some(*level) != last {
                    it.rapid_to(point(it, u0, v0, safety), &name)?;
                }
            }
        }
    } else {
        if number == 84 {
            let pitch = get(it, words, &["ST"])?;
            match pitch {
                Some(p) if it.speed > 0.0 => feed = Some(p.abs() * it.speed),
                None => it.warn("G84 ohne Steigung ST"),
                Some(_) => {}
            }
        }
        it.feed_to(point(it, u0, v0, depth), &name, feed)?;
    }
    if dwell != 0.0 {
        it.dwell(dwell);
    }
    if number == 84 || number == 85 {
        it.feed_to(point(it, u0, v0, safety), &name, feed)?;
    } else {
        it.rapid_to(point(it, u0, v0, safety), &name)?;
    }
    if (retract - safety).abs() > 1e-9 {
        it.rapid_to(point(it, u0, v0, retract), &name)?;
    }
    Ok(())
}

// ----------------------------------------------------------------------- pockets
/// True = clockwise (G2). Default G3.
fn clockwise(ordered: &[Word]) -> bool {
    ordered.iter().any(|w| w.address == "G" && (w.value == "2" || w.value == "02"))
}

fn stepover(it: &mut Interpreter, words: &ByAddress<'_>, r: f64) -> Run<f64> {
    match get(it, words, &["AM"])? {
        Some(am) if am > 0.0 => Ok(am),
        _ => {
            it.note("Fräszyklus ohne Ausräummaß AM: 1,5 x Werkzeugradius angenommen");
            Ok((r * 1.5).max(0.5))
        }
    }
}

fn rotate(cu: f64, cv: f64, u: f64, v: f64, angle_deg: f64) -> Vec2 {
    let a = angle_deg.to_radians();
    let (du, dv) = (u - cu, v - cv);
    (cu + du * a.cos() - dv * a.sin(), cv + du * a.sin() + dv * a.cos())
}

/// Closed path around the origin, starting at (a, 0), counter-clockwise unless `clockwise`.
fn rounded_rect(a: f64, b: f64, rc: f64, clockwise: bool) -> Vec<Vec2> {
    let rc = rc.min(a).min(b).max(0.0);
    let mut pts: Vec<Vec2> = vec![(a, 0.0)];
    let corners =
        [((a - rc, b - rc), 0.0), ((-a + rc, b - rc), 90.0), ((-a + rc, -b + rc), 180.0), ((a - rc, -b + rc), 270.0)];
    for ((cx, cy), start) in corners {
        if rc > 1e-9 {
            for i in 0..10 {
                let ang = (start + 90.0 * f64::from(i) / 9.0_f64).to_radians();
                pts.push((cx + rc * ang.cos(), cy + rc * ang.sin()));
            }
        } else {
            pts.push((cx, cy));
        }
    }
    pts.push((a, 0.0));
    if clockwise {
        pts = pts.into_iter().map(|(x, y)| (x, -y)).collect();
    }
    pts
}

struct MillStart {
    u0: f64,
    v0: f64,
    levels: Vec<f64>,
    fe: Option<f64>,
    fz: Option<f64>,
}

fn mill_levels(it: &mut Interpreter, words: &ByAddress<'_>, name: &str) -> Run<MillStart> {
    let [u0, v0, w0] = uvw(it);
    let (surface, depth, safety, _retract) = levels(it, words, w0)?;
    let levels = infeeds(it, words, surface, depth)?;
    let fe = get(it, words, &["FE"])?;
    let fz = get(it, words, &["FZ"])?;
    it.rapid_to(point(it, u0, v0, safety), name)?;
    Ok(MillStart { u0, v0, levels, fe, fz })
}

fn finish(it: &mut Interpreter, words: &ByAddress<'_>, u0: f64, v0: f64, w0: f64, name: &str) -> Run {
    let (_surface, _depth, _safety, retract) = levels(it, words, w0)?;
    let [u, v, _] = uvw(it);
    it.rapid_to(point(it, u, v, retract), name)?;
    it.rapid_to(point(it, u0, v0, retract), name)?;
    it.note("Fräszyklen enden im Simulator über der Zyklusmitte auf der Rückzugsebene (Annahme)");
    Ok(())
}

fn rect_pocket(it: &mut Interpreter, words: &ByAddress<'_>, ordered: &[Word]) -> Run {
    let name = "G87.1 Rechtecktasche";
    let (length, width) = (get(it, words, &["LI"])?, get(it, words, &["BI"])?);
    let (Some(length), Some(width)) = (length, width) else {
        return Err(cycle_error("G87.1 ohne Taschenlänge LI oder -breite BI"));
    };
    let r = it.tool.radius;
    let (a, b) = (length / 2.0 - r, width / 2.0 - r);
    if a < 0.0 || b < 0.0 {
        return Err(cycle_error("G87.1: Werkzeug ist größer als die Tasche"));
    }
    let corner = or(get(it, words, &["R"])?, 0.0);
    if get(it, words, &["FA"])?.is_some() {
        it.note("G87.1: Fase FA an den Taschenecken wird nicht dargestellt");
    }
    let angle = or(get(it, words, &["DW"])?, 0.0);
    let cw = clockwise(ordered);
    let w_start = uvw(it)[2];
    let start = mill_levels(it, words, name)?;
    let (u0, v0) = (start.u0, start.v0);
    let step = stepover(it, words, r)?;
    let rings = ceil_i64(a.min(b) / step).max(0);
    for level in &start.levels {
        it.feed_to(point(it, u0, v0, *level), name, start.fz)?;
        for k in (0..=rings).rev() {
            let kf = k as f64;
            let (ak, bk) = ((a - kf * step).max(0.0), (b - kf * step).max(0.0));
            let rc = if k == 0 { (corner - r).max(0.0) } else { 0.0 };
            for (x, y) in rounded_rect(ak, bk, rc, cw) {
                let (pu, pv) = rotate(u0, v0, u0 + x, v0 + y, angle);
                it.feed_to(point(it, pu, pv, *level), name, start.fe)?;
            }
        }
        it.feed_to(point(it, u0, v0, *level), name, start.fe)?;
    }
    finish(it, words, u0, v0, w_start, name)
}

fn circle_pocket(it: &mut Interpreter, words: &ByAddress<'_>, ordered: &[Word]) -> Run {
    let name = "G87.2 Kreistasche";
    let Some(diameter) = get(it, words, &["D"])? else {
        return Err(cycle_error("G87.2 ohne Durchmesser D"));
    };
    let r = it.tool.radius;
    let last = diameter / 2.0 - r;
    if last < 0.0 {
        return Err(cycle_error("G87.2: Werkzeug ist größer als die Tasche"));
    }
    let cw = clockwise(ordered);
    let w_start = uvw(it)[2];
    let start = mill_levels(it, words, name)?;
    let (u0, v0) = (start.u0, start.v0);
    let step = stepover(it, words, r)?;
    let first_k = ceil_i64(last / step) - 1;
    let radii: Vec<f64> = (0..=first_k).rev().map(|k| last - k as f64 * step).filter(|radius| *radius > 1e-6).collect();
    for level in &start.levels {
        it.feed_to(point(it, u0, v0, *level), name, start.fz)?;
        for radius in &radii {
            let p = point(it, u0 + radius, v0, *level);
            it.feed_to(p, name, start.fe)?;
            it.arc_to(p, (u0, v0), cw, name, true, start.fe)?;
        }
        it.feed_to(point(it, u0, v0, *level), name, start.fe)?;
    }
    finish(it, words, u0, v0, w_start, name)
}

fn boss(it: &mut Interpreter, words: &ByAddress<'_>, ordered: &[Word]) -> Run {
    let name = "G87.3 Zapfen";
    let Some(diameter) = get(it, words, &["D"])? else {
        return Err(cycle_error("G87.3 ohne Zapfendurchmesser D"));
    };
    let raw = get(it, words, &["DR"])?;
    let r = it.tool.radius;
    let raw = match raw {
        Some(raw) => raw,
        None => {
            it.note("G87.3 ohne Rohteildurchmesser DR: Zapfendurchmesser + 4 x Werkzeugradius angenommen");
            diameter + 4.0 * r
        }
    };
    let cw = clockwise(ordered);
    let last = diameter / 2.0 + r;
    let outer = raw / 2.0 + r;
    let step = stepover(it, words, r)?;
    let count = ceil_i64((outer - last) / step).max(1);
    let mut radii: Vec<f64> = (0..count).map(|k| outer - k as f64 * step).filter(|x| *x > last + 1e-6).collect();
    radii.push(last);
    let [u0, v0, w_start] = uvw(it);
    let (surface, depth, safety, _retract) = levels(it, words, w_start)?;
    let steps = infeeds(it, words, surface, depth)?;
    let (fe, fz) = (get(it, words, &["FE"])?, get(it, words, &["FZ"])?);
    let start_u = u0 + outer + r;
    it.rapid_to(point(it, u0, v0, safety), name)?;
    it.rapid_to(point(it, start_u, v0, safety), name)?;
    for level in &steps {
        it.feed_to(point(it, start_u, v0, *level), name, fz)?;
        for radius in &radii {
            let p = point(it, u0 + radius, v0, *level);
            it.feed_to(p, name, fe)?;
            it.arc_to(p, (u0, v0), cw, name, true, fe)?;
        }
        it.feed_to(point(it, start_u, v0, *level), name, fe)?;
    }
    finish(it, words, u0, v0, w_start, name)
}

fn thread(it: &mut Interpreter, variant: i64, words: &ByAddress<'_>, ordered: &[Word]) -> Run {
    let name = format!("G87.{variant} {}", mill_name(variant).unwrap_or(""));
    let (diameter, pitch) = (get(it, words, &["D"])?, get(it, words, &["ST"])?);
    let (Some(diameter), Some(pitch)) = (diameter, pitch.filter(|p| *p != 0.0)) else {
        return Err(cycle_error(format!("G87.{variant} ohne Durchmesser D oder Steigung ST")));
    };
    let r = it.tool.radius;
    let radius = if variant == 4 { diameter / 2.0 + r } else { diameter / 2.0 - r };
    if radius <= 0.0 {
        return Err(cycle_error(format!("G87.{variant}: Werkzeug ist größer als das Gewinde")));
    }
    let cw = clockwise(ordered);
    let [u0, v0, w_start] = uvw(it);
    let (surface, depth, safety, _retract) = levels(it, words, w_start)?;
    it.note("Gewindefräsen: Schraubenlinie vereinfacht (Anfahren im Viertelkreis nicht dargestellt)");
    it.rapid_to(point(it, u0, v0, safety), &name)?;
    if variant == 4 {
        it.rapid_to(point(it, u0 + radius + 2.0 * r, v0, safety), &name)?;
        it.feed_to(point(it, u0 + radius + 2.0 * r, v0, depth), &name, None)?;
    } else {
        it.feed_to(point(it, u0, v0, depth), &name, None)?;
    }
    it.feed_to(point(it, u0 + radius, v0, depth), &name, None)?;
    let turns = ((surface - depth) / pitch.abs()).max(0.0);
    let steps = ceil_i64(turns * 90.0).max(1);
    let sign = if cw { -1.0 } else { 1.0 };
    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        let angle = 2.0 * std::f64::consts::PI * turns * t * sign;
        let p = point(it, u0 + radius * angle.cos(), v0 + radius * angle.sin(), depth + (surface - depth) * t);
        it.feed_to(p, &name, None)?;
    }
    finish(it, words, u0, v0, w_start, &name)
}

fn contour_pocket(it: &mut Interpreter, words: &ByAddress<'_>) -> Run {
    let name = "G87.9 Konturtasche";
    let [u0, v0, w0] = uvw(it);
    let (_surface, depth, safety, retract) = levels(it, words, w0)?;
    it.rapid_to(point(it, u0, v0, safety), name)?;
    it.contour_pocket = Some((depth, retract));
    it.note("G87.9 Konturtasche: Konturen werden auf Endtiefe gezeigt, das Ausräumen wird nicht berechnet");
    Ok(())
}

// ----------------------------------------------------------------------- point patterns
fn execution(ordered: &[Word], call: Option<SubprogramRef>) -> Run<String> {
    if let Some(reference) = call {
        return Ok(reference.label());
    }
    ordered
        .iter()
        .find(|w| (w.address == "P" || w.address == "PP") && !w.value.is_empty())
        .map(|w| format!("{}{}", w.address, w.value))
        .ok_or_else(|| cycle_error("Punktemuster ohne Ausführungsart (P.., PP.. oder %..)"))
}

fn pattern(
    it: &mut Interpreter,
    variant: i64,
    words: &ByAddress<'_>,
    ordered: &[Word],
    call: Option<SubprogramRef>,
) -> Run {
    let text = execution(ordered, call)?;
    for ignored in ["SL", "SH"] {
        if words.contains_key(ignored) {
            it.note("Schmierhub (SL/SH) wird im Simulator nicht dargestellt");
        }
    }
    for ignored in ["LA", "LB", "LC", "BA", "BB", "BC"] {
        if words.contains_key(ignored) {
            it.note("Drehen/Schwenken von Rundachsen im Punktemuster wird nicht dargestellt");
        }
    }
    let (points, linear, cw, center) = if variant == 1 {
        (line_points(it, words)?, true, false, (0.0, 0.0))
    } else {
        let cw = clockwise(ordered);
        let (points, center) = circle_points(it, words, cw)?;
        let linear = ordered.iter().any(|w| w.address == "G" && (w.value == "1" || w.value == "01"));
        (points, linear, cw, center)
    };
    if points.len() > MAX_PATTERN_POINTS {
        return Err(cycle_error(format!("Punktemuster mit mehr als {MAX_PATTERN_POINTS} Punkten")));
    }
    let name = format!("G89.{variant} {}", pattern_name(variant).unwrap_or(""));
    for (index, (pu, pv)) in points.into_iter().enumerate() {
        let [_, _, w] = uvw(it);
        let target = point(it, pu, pv, w);
        if index == 0 {
            if variant == 2 {
                rapid_linear(it, target, &name)?;
            }
        } else if linear {
            rapid_linear(it, target, &name)?;
        } else {
            it.arc_to(target, center, cw, &name, false, None)?;
        }
        it.execute_text(&text)?;
    }
    Ok(())
}

fn rapid_linear(it: &mut Interpreter, target: Vec3, name: &str) -> Run {
    let previous = it.rapid_variant;
    it.rapid_variant = 2;
    let result = it.rapid_to(target, name);
    it.rapid_variant = previous;
    result
}

fn line_points(it: &Interpreter, words: &ByAddress<'_>) -> Run<Vec<Vec2>> {
    let [u0, v0, _] = uvw(it);
    let count = py_int(or(get(it, words, &["LL"])?, 0.0));
    if count <= 0 {
        return Err(cycle_error("G89.1 ohne Punktezahl LL"));
    }
    let (lx, ly) = (get(it, words, &["LX", "LXI"])?, get(it, words, &["LY", "LYI"])?);
    let (length, angle) = (get(it, words, &["L"])?, get(it, words, &["LW"])?);
    let step = match length {
        Some(length) => {
            let a = or(angle, 0.0).to_radians();
            (length * a.cos(), length * a.sin())
        }
        None => (lx.unwrap_or(0.0), ly.unwrap_or(0.0)),
    };
    let rows = py_int(or(get(it, words, &["BL"])?, 1.0));
    let (bx, by) = (get(it, words, &["BX"])?, get(it, words, &["BY"])?);
    let row = match get(it, words, &["B"])? {
        Some(row_len) => {
            let (bwa, bwi) = (get(it, words, &["BWA", "BW"])?, get(it, words, &["BWI"])?);
            let row_angle = match bwa {
                Some(a) => a,
                None => step.1.atan2(step.0).to_degrees() + or(bwi, 90.0),
            };
            let a = row_angle.to_radians();
            (row_len * a.cos(), row_len * a.sin())
        }
        None => (bx.unwrap_or(0.0), by.unwrap_or(0.0)),
    };
    if rows > 1 && row.0 == 0.0 && row.1 == 0.0 {
        return Err(cycle_error("G89.1: mehrere Reihen (BL) ohne Reihenabstand"));
    }
    let total = usize::try_from(rows.max(0)).unwrap_or(usize::MAX).saturating_mul(usize::try_from(count).unwrap_or(0));
    if total > MAX_PATTERN_POINTS {
        // Same message as the size check of the caller (avoids building a huge list).
        return Err(cycle_error(format!("Punktemuster mit mehr als {MAX_PATTERN_POINTS} Punkten")));
    }
    let mut points = Vec::new();
    for j in 0..rows.max(0) {
        for k in 0..count {
            let (jf, kf) = (j as f64, k as f64);
            points.push((u0 + jf * row.0 + kf * step.0, v0 + jf * row.1 + kf * step.1));
        }
    }
    Ok(points)
}

fn circle_points(it: &Interpreter, words: &ByAddress<'_>, clockwise: bool) -> Run<(Vec<Vec2>, Vec2)> {
    let [u0, v0, _] = uvw(it);
    let cu = get(it, words, &["LX", "LXA"])?;
    let cv = get(it, words, &["LY", "LYA"])?;
    let (cui, cvi) = (get(it, words, &["LXI"])?, get(it, words, &["LYI"])?);
    let center = (cu.unwrap_or(u0 + cui.unwrap_or(0.0)), cv.unwrap_or(v0 + cvi.unwrap_or(0.0)));
    let (radius, start) = (get(it, words, &["R"])?, get(it, words, &["AW"])?);
    let (Some(radius), Some(start)) = (radius, start) else {
        return Err(cycle_error("G89.2 ohne Radius R oder Startwinkel AW"));
    };
    let sign = if clockwise { -1.0 } else { 1.0 };
    let count = get(it, words, &["L"])?;
    let pitch = get(it, words, &["TW"])?;
    let (ewa, ewi) = (get(it, words, &["EWA", "EW"])?, get(it, words, &["EWI"])?);
    let too_many = || cycle_error(format!("Punktemuster mit mehr als {MAX_PATTERN_POINTS} Punkten"));
    let angles: Vec<f64> = if let Some(pitch) = pitch {
        let count = match count {
            Some(count) => py_int(count),
            None => {
                let span = match (ewi, ewa) {
                    (Some(i), _) => i,
                    (None, Some(a)) => a - start,
                    (None, None) => 360.0 - pitch.abs(),
                };
                let ratio = (span.abs() / pitch.abs() + 1e-9).floor();
                if !ratio.is_finite() {
                    return Err(Flow::Error("Punktemuster: Teilungswinkel TW 0 nicht auswertbar".into()));
                }
                py_int(ratio) + 1
            }
        };
        if usize::try_from(count).is_ok_and(|c| c > MAX_PATTERN_POINTS) {
            return Err(too_many());
        }
        (0..count.max(0)).map(|k| start + sign * pitch.abs() * k as f64).collect()
    } else if let Some(count) = count.filter(|c| *c >= 1.0) {
        let n = py_int(count);
        if usize::try_from(n).is_ok_and(|c| c > MAX_PATTERN_POINTS) {
            return Err(too_many());
        }
        if ewa.is_some() || ewi.is_some() {
            let span = match (ewi, ewa) {
                (Some(i), _) => i,
                (None, Some(a)) => a - start,
                (None, None) => 0.0,
            };
            let divisor = (n - 1).max(1) as f64;
            (0..n).map(|k| start + span * k as f64 / divisor).collect()
        } else {
            (0..n).map(|k| start + sign * 360.0 * k as f64 / n as f64).collect()
        }
    } else {
        return Err(cycle_error("G89.2: Punkteaufteilung unvollständig (TW, EW oder L fehlt)"));
    };
    let points = angles
        .iter()
        .map(|a| (center.0 + radius * a.to_radians().cos(), center.1 + radius * a.to_radians().sin()))
        .collect();
    Ok((points, center))
}
