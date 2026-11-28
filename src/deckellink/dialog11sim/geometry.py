"""Geometry helpers: working planes, arcs, coordinate transformation, path offset."""

from __future__ import annotations

import math
from dataclasses import dataclass

Vec3 = tuple[float, float, float]
Vec2 = tuple[float, float]

ARC_STEP_DEG = 4.0
EPS = 1e-9


@dataclass(frozen=True)
class Plane:
    """Working plane: indices of main axis, second axis and tool (infeed) axis in (X, Y, Z)."""

    code: int  # 17, 18, 19
    main: int
    second: int
    tool: int

    @property
    def label(self) -> str:
        names = "XYZ"
        return f"G{self.code} ({names[self.main]}{names[self.second]}-Ebene, Zustellung {names[self.tool]})"

    def to_plane(self, p: Vec3) -> tuple[float, float, float]:
        return p[self.main], p[self.second], p[self.tool]

    def from_plane(self, u: float, v: float, w: float) -> Vec3:
        out = [0.0, 0.0, 0.0]
        out[self.main], out[self.second], out[self.tool] = u, v, w
        return out[0], out[1], out[2]


# G18/G19 axis assignment is an assumption of the simulator (see docs/SIMULATOR.md).
PLANES = {17: Plane(17, 0, 1, 2), 18: Plane(18, 0, 2, 1), 19: Plane(19, 1, 2, 0)}


def distance(a: Vec3, b: Vec3) -> float:
    return math.dist(a, b)


def lerp(a: Vec3, b: Vec3, t: float) -> Vec3:
    return a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t


def arc_points(
    plane: Plane,
    start: Vec3,
    end: Vec3,
    center_uv: Vec2,
    clockwise: bool,
    *,
    full_circle: bool = False,
    sweep_override: float | None = None,
) -> list[Vec3]:
    """Points along an arc (helix if the tool axis changes), start excluded, end included."""
    su, sv, sw = plane.to_plane(start)
    eu, ev, ew = plane.to_plane(end)
    cu, cv = center_uv
    r_start = math.hypot(su - cu, sv - cv)
    r_end = math.hypot(eu - cu, ev - cv)
    a0 = math.atan2(sv - cv, su - cu)
    a1 = math.atan2(ev - cv, eu - cu)
    if sweep_override is not None:
        sweep = math.radians(sweep_override)
    elif full_circle or math.dist((su, sv), (eu, ev)) <= 1e-9:
        sweep = -2 * math.pi if clockwise else 2 * math.pi
    else:
        sweep = a1 - a0
        if clockwise:
            while sweep >= 0:
                sweep -= 2 * math.pi
            while sweep <= -2 * math.pi:
                sweep += 2 * math.pi
        else:
            while sweep <= 0:
                sweep += 2 * math.pi
            while sweep >= 2 * math.pi:
                sweep -= 2 * math.pi
    steps = max(2, math.ceil(abs(math.degrees(sweep)) / ARC_STEP_DEG))
    points: list[Vec3] = []
    for i in range(1, steps + 1):
        t = i / steps
        angle = a0 + sweep * t
        radius = r_start + (r_end - r_start) * t
        w = sw + (ew - sw) * t
        points.append(plane.from_plane(cu + radius * math.cos(angle), cv + radius * math.sin(angle), w))
    if sweep_override is None:
        points[-1] = end
    return points


def circle_center_from_radius(start: Vec2, end: Vec2, radius: float, clockwise: bool) -> Vec2 | None:
    """Centre for an arc with given radius; positive radius = arc up to 180°, negative = longer arc."""
    dx, dy = end[0] - start[0], end[1] - start[1]
    chord = math.hypot(dx, dy)
    r = abs(radius)
    if chord < EPS or chord > 2 * r + 1e-6:
        return None
    mx, my = (start[0] + end[0]) / 2, (start[1] + end[1]) / 2
    h = math.sqrt(max(r * r - (chord / 2) ** 2, 0.0))
    nx, ny = -dy / chord, dx / chord  # left normal of the chord
    # clockwise short arc has the centre on the right of the chord
    side = -1.0 if clockwise else 1.0
    if radius < 0:
        side = -side
    return mx + side * h * nx, my + side * h * ny


@dataclass(frozen=True)
class Transform:
    """Program coordinates -> workpiece coordinates (offset, rotation in plane, mirror, scale)."""

    offset: Vec3 = (0.0, 0.0, 0.0)
    rotation_deg: float = 0.0
    plane: Plane = PLANES[17]
    mirror: tuple[float | None, float | None, float | None] = (None, None, None)
    scale: Vec3 = (1.0, 1.0, 1.0)

    def apply(self, p: Vec3) -> Vec3:
        q = list(p)
        for axis in range(3):
            m = self.mirror[axis]
            if m is not None:
                q[axis] = 2 * m - q[axis]
            q[axis] *= self.scale[axis]
        u, v, w = self.plane.to_plane((q[0], q[1], q[2]))
        a = math.radians(self.rotation_deg)
        ru, rv = u * math.cos(a) - v * math.sin(a), u * math.sin(a) + v * math.cos(a)
        r = self.plane.from_plane(ru, rv, w)
        return r[0] + self.offset[0], r[1] + self.offset[1], r[2] + self.offset[2]

    def invert(self, p: Vec3) -> Vec3:
        r = (p[0] - self.offset[0], p[1] - self.offset[1], p[2] - self.offset[2])
        u, v, w = self.plane.to_plane(r)
        a = math.radians(-self.rotation_deg)
        ru, rv = u * math.cos(a) - v * math.sin(a), u * math.sin(a) + v * math.cos(a)
        q = list(self.plane.from_plane(ru, rv, w))
        for axis in range(3):
            factor = self.scale[axis]
            q[axis] = q[axis] / factor if abs(factor) > EPS else 0.0
            m = self.mirror[axis]
            if m is not None:
                q[axis] = 2 * m - q[axis]
        return q[0], q[1], q[2]

    @property
    def reverses_direction(self) -> bool:
        """Odd number of mirrorings in the working plane reverse G2/G3 and G41/G42."""
        count = 0
        for axis in (self.plane.main, self.plane.second):
            flip = (self.mirror[axis] is not None) != (self.scale[axis] < 0)
            count += int(flip)
        return count % 2 == 1


def offset_polyline(
    points: list[Vec3], sources: list[int], plane: Plane, radius: float, left: bool
) -> tuple[list[Vec3], list[int]]:
    """Tool centre path for a contour (radius compensation).

    ``sources[i]`` names the contour element that ends at ``points[i]``. The
    result carries the same information for every output point. Inner corners:
    intersection of the offset lines. Outer corners: transition arc around the
    contour corner (behaviour of G60 according to the training documentation).
    """
    pts = [points[0]]
    src = [sources[0]]
    for p, s_index in zip(points[1:], sources[1:], strict=True):
        if math.dist(plane.to_plane(p)[:2], plane.to_plane(pts[-1])[:2]) > 1e-7:
            pts.append(p)
            src.append(s_index)
        else:
            pts[-1] = p
            src[-1] = s_index
    if len(pts) < 2 or radius <= 0:
        return list(pts), list(src)
    side = 1.0 if left else -1.0

    def normal(a: Vec3, b: Vec3) -> Vec2:
        au, av, _ = plane.to_plane(a)
        bu, bv, _ = plane.to_plane(b)
        length = math.hypot(bu - au, bv - av)
        return (-(bv - av) / length * side, (bu - au) / length * side)

    def shifted(p: Vec3, n: Vec2) -> Vec3:
        u, v, w = plane.to_plane(p)
        return plane.from_plane(u + n[0] * radius, v + n[1] * radius, w)

    out: list[Vec3] = []
    out_src: list[int] = []
    normals = [normal(pts[i], pts[i + 1]) for i in range(len(pts) - 1)]
    out.append(shifted(pts[0], normals[0]))
    out_src.append(src[0])
    for i in range(1, len(pts) - 1):
        n_in, n_out = normals[i - 1], normals[i]
        a_end = shifted(pts[i], n_in)
        b_start = shifted(pts[i], n_out)
        pu, pv, pw = plane.to_plane(pts[i])
        d_in = (n_in[1] * side, -n_in[0] * side)
        d_out = (n_out[1] * side, -n_out[0] * side)
        cross = d_in[0] * d_out[1] - d_in[1] * d_out[0]
        dot = d_in[0] * d_out[0] + d_in[1] * d_out[1]
        if abs(cross) < 1e-9 and dot > 0:
            out.append(a_end)
            out_src.append(src[i])
            continue
        inner = cross * side > 1e-9  # exact reversal (cross ~ 0) counts as outer corner
        if inner:
            hit = _intersect(plane.to_plane(a_end)[:2], d_in, plane.to_plane(b_start)[:2], d_out)
            if hit is not None and math.dist(hit, (pu, pv)) <= 4 * radius:
                out.append(plane.from_plane(hit[0], hit[1], pw))
                out_src.append(src[i])
            else:  # nearly reversing direction: connect both offset points
                out.extend([a_end, b_start])
                out_src.extend([src[i], src[i + 1]])
        else:
            out.append(a_end)
            out_src.append(src[i])
            corner = arc_points(plane, a_end, b_start, (pu, pv), clockwise=left)
            out.extend(corner)
            out_src.extend([src[i + 1]] * len(corner))
    out.append(shifted(pts[-1], normals[-1]))
    out_src.append(src[-1])
    return out, out_src


def _intersect(p: Vec2, d: Vec2, q: Vec2, e: Vec2) -> Vec2 | None:
    denom = d[0] * e[1] - d[1] * e[0]
    if abs(denom) < 1e-12:
        return None
    t = ((q[0] - p[0]) * e[1] - (q[1] - p[1]) * e[0]) / denom
    return p[0] + d[0] * t, p[1] + d[1] * t


def direction(plane: Plane, a: Vec3, b: Vec3) -> Vec2:
    au, av, _ = plane.to_plane(a)
    bu, bv, _ = plane.to_plane(b)
    length = math.hypot(bu - au, bv - av)
    if length < EPS:
        return 1.0, 0.0
    return (bu - au) / length, (bv - av) / length
