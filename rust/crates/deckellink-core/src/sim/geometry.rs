//! Geometry helpers: working planes, arcs, coordinate transformation, path offset.

pub type Vec3 = [f64; 3];
pub type Vec2 = (f64, f64);

pub const ARC_STEP_DEG: f64 = 4.0;
pub const EPS: f64 = 1e-9;

/// Read a coordinate by axis index (0..=2); other indices read 0.
pub fn axis(p: &Vec3, index: usize) -> f64 {
    p.get(index).copied().unwrap_or(0.0)
}

/// Set a coordinate by axis index (0..=2); other indices are ignored.
pub fn set_axis(p: &mut Vec3, index: usize, value: f64) {
    if let Some(slot) = p.get_mut(index) {
        *slot = value;
    }
}

/// Working plane: indices of main axis, second axis and tool (infeed) axis in (X, Y, Z).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plane {
    pub code: i64,
    pub main: usize,
    pub second: usize,
    pub tool: usize,
}

// G18/G19 axis assignment is an assumption of the simulator (see docs/SIMULATOR.md).
pub const G17: Plane = Plane { code: 17, main: 0, second: 1, tool: 2 };
pub const G18: Plane = Plane { code: 18, main: 0, second: 2, tool: 1 };
pub const G19: Plane = Plane { code: 19, main: 1, second: 2, tool: 0 };

/// `PLANES[code]` of the Python version.
pub fn plane_for(code: i64) -> Option<Plane> {
    match code {
        17 => Some(G17),
        18 => Some(G18),
        19 => Some(G19),
        _ => None,
    }
}

const AXIS_NAMES: [&str; 3] = ["X", "Y", "Z"];

pub fn axis_name(index: usize) -> &'static str {
    AXIS_NAMES.get(index).copied().unwrap_or("?")
}

impl Plane {
    pub fn label(&self) -> String {
        format!(
            "G{} ({}{}-Ebene, Zustellung {})",
            self.code,
            axis_name(self.main),
            axis_name(self.second),
            axis_name(self.tool)
        )
    }

    pub fn to_plane(&self, p: Vec3) -> Vec3 {
        [axis(&p, self.main), axis(&p, self.second), axis(&p, self.tool)]
    }

    pub fn from_plane(&self, u: f64, v: f64, w: f64) -> Vec3 {
        let mut out = [0.0; 3];
        set_axis(&mut out, self.main, u);
        set_axis(&mut out, self.second, v);
        set_axis(&mut out, self.tool, w);
        out
    }
}

/// Python `math.dist` / `math.hypot` on a few values (scaled to avoid overflow).
pub fn hypot_n(values: &[f64]) -> f64 {
    let max = values.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
    if max == 0.0 || !max.is_finite() {
        return max;
    }
    if values.len() == 2 {
        if let (Some(a), Some(b)) = (values.first(), values.get(1)) {
            return a.hypot(*b);
        }
    }
    let sum: f64 = values.iter().map(|v| (v / max) * (v / max)).sum();
    max * sum.sqrt()
}

pub fn distance(a: Vec3, b: Vec3) -> f64 {
    hypot_n(&[a[0] - b[0], a[1] - b[1], a[2] - b[2]])
}

pub fn dist2(a: Vec2, b: Vec2) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

pub fn lerp(a: Vec3, b: Vec3, t: f64) -> Vec3 {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// Python `math.ceil` for finite values (saturating).
#[allow(clippy::cast_possible_truncation)]
pub fn ceil_i64(value: f64) -> i64 {
    value.ceil() as i64
}

/// Points along an arc (helix if the tool axis changes), start excluded, end included.
pub fn arc_points(
    plane: &Plane,
    start: Vec3,
    end: Vec3,
    center_uv: Vec2,
    clockwise: bool,
    full_circle: bool,
    sweep_override: Option<f64>,
) -> Vec<Vec3> {
    use std::f64::consts::PI;
    let [su, sv, sw] = plane.to_plane(start);
    let [eu, ev, ew] = plane.to_plane(end);
    let (cu, cv) = center_uv;
    let r_start = (su - cu).hypot(sv - cv);
    let r_end = (eu - cu).hypot(ev - cv);
    let a0 = (sv - cv).atan2(su - cu);
    let a1 = (ev - cv).atan2(eu - cu);
    let sweep = if let Some(degrees) = sweep_override {
        degrees.to_radians()
    } else if full_circle || dist2((su, sv), (eu, ev)) <= 1e-9 {
        if clockwise {
            -2.0 * PI
        } else {
            2.0 * PI
        }
    } else {
        let mut sweep = a1 - a0;
        if clockwise {
            while sweep >= 0.0 {
                sweep -= 2.0 * PI;
            }
            while sweep <= -2.0 * PI {
                sweep += 2.0 * PI;
            }
        } else {
            while sweep <= 0.0 {
                sweep += 2.0 * PI;
            }
            while sweep >= 2.0 * PI {
                sweep -= 2.0 * PI;
            }
        }
        sweep
    };
    let steps = ceil_i64(sweep.to_degrees().abs() / ARC_STEP_DEG).max(2);
    let mut points = Vec::new();
    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        let angle = a0 + sweep * t;
        let radius = r_start + (r_end - r_start) * t;
        let w = sw + (ew - sw) * t;
        points.push(plane.from_plane(cu + radius * angle.cos(), cv + radius * angle.sin(), w));
    }
    if sweep_override.is_none() {
        if let Some(last) = points.last_mut() {
            *last = end;
        }
    }
    points
}

/// Centre for an arc with given radius; positive radius = arc up to 180°, negative = longer arc.
pub fn circle_center_from_radius(start: Vec2, end: Vec2, radius: f64, clockwise: bool) -> Option<Vec2> {
    let (dx, dy) = (end.0 - start.0, end.1 - start.1);
    let chord = dx.hypot(dy);
    let r = radius.abs();
    if chord < EPS || chord > 2.0 * r + 1e-6 {
        return None;
    }
    let (mx, my) = ((start.0 + end.0) / 2.0, (start.1 + end.1) / 2.0);
    let h = (r * r - (chord / 2.0).powi(2)).max(0.0).sqrt();
    let (nx, ny) = (-dy / chord, dx / chord);
    let mut side = if clockwise { -1.0 } else { 1.0 };
    if radius < 0.0 {
        side = -side;
    }
    Some((mx + side * h * nx, my + side * h * ny))
}

/// Program coordinates -> workpiece coordinates (offset, rotation in plane, mirror, scale).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub offset: Vec3,
    pub rotation_deg: f64,
    pub plane: Plane,
    pub mirror: [Option<f64>; 3],
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Self { offset: [0.0; 3], rotation_deg: 0.0, plane: G17, mirror: [None; 3], scale: [1.0; 3] }
    }
}

impl Transform {
    pub fn apply(&self, p: Vec3) -> Vec3 {
        let mut q = p;
        for ((value, mirror), scale) in q.iter_mut().zip(self.mirror.iter()).zip(self.scale.iter()) {
            if let Some(m) = mirror {
                *value = 2.0 * m - *value;
            }
            *value *= scale;
        }
        let [u, v, w] = self.plane.to_plane(q);
        let a = self.rotation_deg.to_radians();
        let (ru, rv) = (u * a.cos() - v * a.sin(), u * a.sin() + v * a.cos());
        let r = self.plane.from_plane(ru, rv, w);
        [r[0] + self.offset[0], r[1] + self.offset[1], r[2] + self.offset[2]]
    }

    pub fn invert(&self, p: Vec3) -> Vec3 {
        let r = [p[0] - self.offset[0], p[1] - self.offset[1], p[2] - self.offset[2]];
        let [u, v, w] = self.plane.to_plane(r);
        let a = (-self.rotation_deg).to_radians();
        let (ru, rv) = (u * a.cos() - v * a.sin(), u * a.sin() + v * a.cos());
        let mut q = self.plane.from_plane(ru, rv, w);
        for ((value, mirror), factor) in q.iter_mut().zip(self.mirror.iter()).zip(self.scale.iter()) {
            *value = if factor.abs() > EPS { *value / factor } else { 0.0 };
            if let Some(m) = mirror {
                *value = 2.0 * m - *value;
            }
        }
        q
    }

    /// Odd number of mirrorings in the working plane reverse G2/G3 and G41/G42.
    pub fn reverses_direction(&self) -> bool {
        let mut count = 0;
        for index in [self.plane.main, self.plane.second] {
            let mirrored = self.mirror.get(index).is_some_and(Option::is_some);
            let negative = axis(&self.scale, index) < 0.0;
            if mirrored != negative {
                count += 1;
            }
        }
        count % 2 == 1
    }
}

fn uv(plane: &Plane, p: Vec3) -> Vec2 {
    let [u, v, _] = plane.to_plane(p);
    (u, v)
}

fn intersect(p: Vec2, d: Vec2, q: Vec2, e: Vec2) -> Option<Vec2> {
    let denom = d.0 * e.1 - d.1 * e.0;
    if denom.abs() < 1e-12 {
        return None;
    }
    let t = ((q.0 - p.0) * e.1 - (q.1 - p.1) * e.0) / denom;
    Some((p.0 + d.0 * t, p.1 + d.1 * t))
}

/// Tool centre path for a contour (radius compensation).
///
/// `sources[i]` names the contour element that ends at `points[i]`. The result carries
/// the same information for every output point. Inner corners: intersection of the
/// offset lines. Outer corners: transition arc around the contour corner (behaviour of
/// G60 according to the training documentation).
pub fn offset_polyline(
    points: &[Vec3],
    sources: &[usize],
    plane: &Plane,
    radius: f64,
    left: bool,
) -> (Vec<Vec3>, Vec<usize>) {
    let mut pts: Vec<Vec3> = Vec::new();
    let mut src: Vec<usize> = Vec::new();
    for (p, s) in points.iter().zip(sources.iter()) {
        match (pts.last_mut(), src.last_mut()) {
            (Some(last), Some(last_src)) if dist2(uv(plane, *p), uv(plane, *last)) <= 1e-7 => {
                *last = *p;
                *last_src = *s;
            }
            _ => {
                pts.push(*p);
                src.push(*s);
            }
        }
    }
    if pts.len() < 2 || radius <= 0.0 {
        return (pts, src);
    }
    let side = if left { 1.0 } else { -1.0 };
    let normal = |a: Vec3, b: Vec3| -> Vec2 {
        let (au, av) = uv(plane, a);
        let (bu, bv) = uv(plane, b);
        let length = (bu - au).hypot(bv - av);
        (-(bv - av) / length * side, (bu - au) / length * side)
    };
    let shifted = |p: Vec3, n: Vec2| -> Vec3 {
        let [u, v, w] = plane.to_plane(p);
        plane.from_plane(u + n.0 * radius, v + n.1 * radius, w)
    };
    let normals: Vec<Vec2> = pts.iter().zip(pts.iter().skip(1)).map(|(a, b)| normal(*a, *b)).collect();
    let mut out: Vec<Vec3> = Vec::new();
    let mut out_src: Vec<usize> = Vec::new();
    let (Some(first), Some(first_normal), Some(first_src)) = (pts.first(), normals.first(), src.first()) else {
        return (pts.clone(), src.clone());
    };
    out.push(shifted(*first, *first_normal));
    out_src.push(*first_src);
    for i in 1..pts.len() - 1 {
        let (Some(n_in), Some(n_out), Some(point), Some(s_here), Some(s_next)) =
            (normals.get(i - 1), normals.get(i), pts.get(i), src.get(i), src.get(i + 1))
        else {
            continue;
        };
        let (n_in, n_out, point) = (*n_in, *n_out, *point);
        let a_end = shifted(point, n_in);
        let b_start = shifted(point, n_out);
        let [pu, pv, pw] = plane.to_plane(point);
        let d_in = (n_in.1 * side, -n_in.0 * side);
        let d_out = (n_out.1 * side, -n_out.0 * side);
        let cross = d_in.0 * d_out.1 - d_in.1 * d_out.0;
        let dot = d_in.0 * d_out.0 + d_in.1 * d_out.1;
        if cross.abs() < 1e-9 && dot > 0.0 {
            out.push(a_end);
            out_src.push(*s_here);
            continue;
        }
        // exact reversal (cross ~ 0) counts as outer corner: no rounding-dependent sign
        let inner = cross * side > 1e-9;
        if inner {
            let hit = intersect(uv(plane, a_end), d_in, uv(plane, b_start), d_out);
            match hit {
                Some(hit) if dist2(hit, (pu, pv)) <= 4.0 * radius => {
                    out.push(plane.from_plane(hit.0, hit.1, pw));
                    out_src.push(*s_here);
                }
                _ => {
                    out.extend([a_end, b_start]);
                    out_src.extend([*s_here, *s_next]);
                }
            }
        } else {
            out.push(a_end);
            out_src.push(*s_here);
            let corner = arc_points(plane, a_end, b_start, (pu, pv), left, false, None);
            out_src.extend(std::iter::repeat_n(*s_next, corner.len()));
            out.extend(corner);
        }
    }
    if let (Some(last), Some(last_normal), Some(last_src)) = (pts.last(), normals.last(), src.last()) {
        out.push(shifted(*last, *last_normal));
        out_src.push(*last_src);
    }
    (out, out_src)
}

pub fn direction(plane: &Plane, a: Vec3, b: Vec3) -> Vec2 {
    let (au, av) = uv(plane, a);
    let (bu, bv) = uv(plane, b);
    let length = (bu - au).hypot(bv - av);
    if length < EPS {
        return (1.0, 0.0);
    }
    ((bu - au) / length, (bv - av) / length)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_roundtrip_and_arc() {
        let t = Transform {
            offset: [10.0, 5.0, 0.0],
            rotation_deg: 30.0,
            plane: G17,
            mirror: [Some(1.0), None, None],
            scale: [2.0, 1.0, 1.0],
        };
        let p = [3.0, 4.0, 5.0];
        let back = t.invert(t.apply(p));
        assert!(distance(p, back) < 1e-12);
        assert!(t.reverses_direction());
        let pts = arc_points(&G17, [10.0, 0.0, 0.0], [0.0, 10.0, 0.0], (0.0, 0.0), false, false, None);
        assert_eq!(pts.len(), 23);
        assert_eq!(pts.last().copied(), Some([0.0, 10.0, 0.0]));
        assert_eq!(circle_center_from_radius((0.0, 0.0), (10.0, 0.0), 5.0, false), Some((5.0, 0.0)));
    }
}
