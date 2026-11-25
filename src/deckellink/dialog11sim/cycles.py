"""Machining cycles G81-G86, G87.x and G89.x (simplified tool paths).

Addresses and sequences follow the training documentation (module 8). Where
the documentation leaves details open (step-over without AM, retract during
chip breaking, end position of pocket cycles ...) the simulator uses a
documented assumption and reports it as a note.
"""

from __future__ import annotations

import math
from typing import TYPE_CHECKING

from deckellink.dialog11sim.expressions import ExpressionError
from deckellink.dialog11sim.geometry import Vec3
from deckellink.dialog11sim.model import Severity

if TYPE_CHECKING:
    from deckellink.dialog11sim.interpreter import Interpreter
    from deckellink.dialog11sim.syntax import SubprogramRef, Word

DRILL_NAMES = {
    81: "Bohren",
    82: "Bohren mit Spanbrechen",
    83: "Tieflochbohren",
    84: "Gewindebohren",
    85: "Reiben",
    86: "Ausdrehen",
}
MILL_NAMES = {
    1: "Rechtecktasche",
    2: "Kreistasche",
    3: "Zapfen",
    4: "Außengewinde fräsen",
    5: "Innengewinde fräsen",
    9: "Konturtasche",
}
PATTERN_NAMES = {1: "Punktemuster Gerade", 2: "Punktemuster Teilkreis"}
MAX_LEVELS = 500
MAX_PATTERN_POINTS = 5000


class CycleError(ExpressionError):
    """Cycle block incomplete or inconsistent."""


def run_cycle(
    it: Interpreter,
    number: int,
    variant: int | None,
    words: dict[str, Word],
    ordered: list[Word],
    call: SubprogramRef | None = None,
) -> None:
    try:
        if number in DRILL_NAMES:
            _drill(it, number, words, ordered)
        elif number == 87:
            if variant not in MILL_NAMES:
                raise CycleError(f"G87.{variant}: unbekannte Zyklusvariante")
            {1: _rect_pocket, 2: _circle_pocket, 3: _boss, 4: _thread, 5: _thread, 9: _contour_pocket}[variant](
                it, variant, words, ordered
            )
        elif number == 89:
            if variant not in PATTERN_NAMES:
                raise CycleError(f"G89.{variant}: unbekannte Musterart")
            _pattern(it, variant, words, ordered, call)
        else:
            raise CycleError(f"G{number}: kein Zyklus")
    except CycleError as exc:
        it._msg(Severity.ERROR, str(exc))


# ----------------------------------------------------------------------- helpers
def _get(it: Interpreter, words: dict[str, Word], *names: str) -> float | None:
    for name in names:
        word = words.get(name)
        if word is not None:
            return it.value(word)
    return None


def _levels(it: Interpreter, words: dict[str, Word], w0: float) -> tuple[float, float, float, float]:
    """(surface, depth, safety, retract) on the tool axis, program coordinates."""
    oa, oi = _get(it, words, "OA"), _get(it, words, "OI")
    surface = oa if oa is not None else (w0 - abs(oi) if oi is not None else w0)
    if oa is None and oi is None:
        it.note("Zyklus ohne Oberfläche (OA/OI): Inkrementalangaben beziehen sich auf die Werkzeugposition")
    ta, ti = _get(it, words, "TA"), _get(it, words, "TI")
    if ta is not None:
        depth = ta
    elif ti is not None:
        depth = surface - abs(ti)
    else:
        raise CycleError("Zyklus ohne Tiefe (TA oder TI)")
    sa, si = _get(it, words, "SA"), _get(it, words, "SI")
    safety = sa if sa is not None else (surface + abs(si) if si is not None else surface)
    ea, ei = _get(it, words, "EA"), _get(it, words, "EI")
    retract = ea if ea is not None else (surface + abs(ei) if ei is not None else safety)
    if ea is None and ei is None:
        it.warn("Zyklus ohne Rückzugsebene (EA/EI): Rückzug nur auf Sicherheitsabstand - Kollisionsgefahr")
    if depth > surface:
        it.warn("Zyklus: Tiefe liegt oberhalb der Oberfläche")
    return surface, depth, safety, retract


def _dwell_and_feed(it: Interpreter, ordered: list[Word]) -> tuple[float, float | None]:
    dwell = 0.0
    feed: float | None = None
    for index, word in enumerate(ordered):
        if word.address == "F":
            previous = ordered[index - 1] if index else None
            if previous is not None and previous.address == "G" and previous.value in ("4", "04"):
                dwell = it.value(word)
            else:
                feed = it.value(word)
    return dwell, feed


def _point(it: Interpreter, u: float, v: float, w: float) -> Vec3:
    return it.plane.from_plane(u, v, w)


def _infeeds(it: Interpreter, words: dict[str, Word], start: float, depth: float) -> list[float]:
    mi = _get(it, words, "MI")
    di = _get(it, words, "DI") or 0.0
    total = start - depth
    if mi is None or mi <= 0 or total <= 0:
        return [depth]
    levels: list[float] = []
    step = abs(mi)
    level = start
    minimum = max(abs(di), step * 0.1, 0.01)
    while level - step > depth + 1e-9 and len(levels) < MAX_LEVELS:
        level -= step
        levels.append(level)
        step = max(step - abs(di), minimum)
    levels.append(depth)
    return levels


# ----------------------------------------------------------------------- drilling
def _drill(it: Interpreter, number: int, words: dict[str, Word], ordered: list[Word]) -> None:
    name = f"G{number} {DRILL_NAMES[number]}"
    u0, v0, w0 = it.plane.to_plane(it.position)
    surface, depth, safety, retract = _levels(it, words, w0)
    dwell, feed = _dwell_and_feed(it, ordered)
    fz = _get(it, words, "FZ")
    feed = fz if fz is not None else feed
    it.rapid_to(_point(it, u0, v0, safety), note=name)
    if number in (82, 83):
        levels = _infeeds(it, words, surface, depth)
        if number == 82:
            it.note("G82 Spanbrechen: kurzer Rückzug nach jeder Zustellung wird nicht dargestellt")
            for level in levels:
                it.feed_to(_point(it, u0, v0, level), name, feed)
        else:
            it.note("G83: Wiederanfahren im Eilgang bis 1 mm über den erreichten Bohrgrund (Annahme)")
            reached = surface
            for level in levels:
                if reached < surface:
                    it.rapid_to(_point(it, u0, v0, reached + 1.0), note=name)
                it.feed_to(_point(it, u0, v0, level), name, feed)
                reached = level
                if level != levels[-1]:
                    it.rapid_to(_point(it, u0, v0, safety), note=name)
    else:
        if number == 84:
            pitch = _get(it, words, "ST")
            if pitch is not None and it.speed > 0:
                feed = abs(pitch) * it.speed
            elif pitch is None:
                it.warn("G84 ohne Steigung ST")
        it.feed_to(_point(it, u0, v0, depth), name, feed)
    if dwell:
        it.dwell(dwell)
    if number in (84, 85):
        it.feed_to(_point(it, u0, v0, safety), name, feed)
    else:
        it.rapid_to(_point(it, u0, v0, safety), note=name)
    if abs(retract - safety) > 1e-9:
        it.rapid_to(_point(it, u0, v0, retract), note=name)


# ----------------------------------------------------------------------- pockets
def _direction(ordered: list[Word]) -> bool:
    """True = clockwise (G2). Default G3."""
    return any(w.address == "G" and w.value in ("2", "02") for w in ordered)


def _stepover(it: Interpreter, words: dict[str, Word], r: float) -> float:
    am = _get(it, words, "AM")
    if am is None or am <= 0:
        it.note("Fräszyklus ohne Ausräummaß AM: 1,5 x Werkzeugradius angenommen")
        return max(r * 1.5, 0.5)
    return am


def _rotate(cu: float, cv: float, u: float, v: float, angle_deg: float) -> tuple[float, float]:
    a = math.radians(angle_deg)
    du, dv = u - cu, v - cv
    return cu + du * math.cos(a) - dv * math.sin(a), cv + du * math.sin(a) + dv * math.cos(a)


def _rounded_rect(a: float, b: float, rc: float, clockwise: bool) -> list[tuple[float, float]]:
    """Closed path around the origin, starting at (a, 0), counter-clockwise unless ``clockwise``."""
    rc = max(0.0, min(rc, a, b))
    pts: list[tuple[float, float]] = [(a, 0.0)]
    corners = [
        ((a - rc, b - rc), 0.0),
        ((-a + rc, b - rc), 90.0),
        ((-a + rc, -b + rc), 180.0),
        ((a - rc, -b + rc), 270.0),
    ]
    for (cx, cy), start in corners:
        if rc > 1e-9:
            for i in range(0, 10):
                ang = math.radians(start + 90.0 * i / 9)
                pts.append((cx + rc * math.cos(ang), cy + rc * math.sin(ang)))
        else:
            pts.append((cx, cy))
    pts.append((a, 0.0))
    if clockwise:
        pts = [(x, -y) for x, y in pts]
    return pts


def _mill_levels(
    it: Interpreter, words: dict[str, Word], name: str
) -> tuple[float, float, float, list[float], float, float | None, float | None]:
    u0, v0, w0 = it.plane.to_plane(it.position)
    surface, depth, safety, retract = _levels(it, words, w0)
    del retract
    levels = _infeeds(it, words, surface, depth)
    fe = _get(it, words, "FE")
    fz = _get(it, words, "FZ")
    it.rapid_to(_point(it, u0, v0, safety), note=name)
    return u0, v0, safety, levels, surface, fe, fz


def _finish(it: Interpreter, words: dict[str, Word], u0: float, v0: float, w0: float, name: str) -> None:
    surface_w = w0
    _surface, _depth, _safety, retract = _levels(it, words, surface_w)
    u, v, _ = it.plane.to_plane(it.position)
    it.rapid_to(_point(it, u, v, retract), note=name)
    it.rapid_to(_point(it, u0, v0, retract), note=name)
    it.note("Fräszyklen enden im Simulator über der Zyklusmitte auf der Rückzugsebene (Annahme)")


def _rect_pocket(it: Interpreter, _variant: int, words: dict[str, Word], ordered: list[Word]) -> None:
    name = "G87.1 Rechtecktasche"
    length, width = _get(it, words, "LI"), _get(it, words, "BI")
    if length is None or width is None:
        raise CycleError("G87.1 ohne Taschenlänge LI oder -breite BI")
    r = it.tool.radius
    a, b = length / 2 - r, width / 2 - r
    if a < 0 or b < 0:
        raise CycleError("G87.1: Werkzeug ist größer als die Tasche")
    corner = _get(it, words, "R") or 0.0
    if _get(it, words, "FA") is not None:
        it.note("G87.1: Fase FA an den Taschenecken wird nicht dargestellt")
    angle = _get(it, words, "DW") or 0.0
    clockwise = _direction(ordered)
    w_start = it.plane.to_plane(it.position)[2]
    u0, v0, _safety, levels, _surface, fe, fz = _mill_levels(it, words, name)
    step = _stepover(it, words, r)
    rings = max(0, math.ceil(min(a, b) / step))
    for level in levels:
        it.feed_to(_point(it, u0, v0, level), name, fz)
        for k in range(rings, -1, -1):
            ak, bk = max(a - k * step, 0.0), max(b - k * step, 0.0)
            rc = max(corner - r, 0.0) if k == 0 else 0.0
            for x, y in _rounded_rect(ak, bk, rc, clockwise):
                pu, pv = _rotate(u0, v0, u0 + x, v0 + y, angle)
                it.feed_to(_point(it, pu, pv, level), name, fe)
        it.feed_to(_point(it, u0, v0, level), name, fe)
    _finish(it, words, u0, v0, w_start, name)


def _circle_pocket(it: Interpreter, _variant: int, words: dict[str, Word], ordered: list[Word]) -> None:
    name = "G87.2 Kreistasche"
    diameter = _get(it, words, "D")
    if diameter is None:
        raise CycleError("G87.2 ohne Durchmesser D")
    r = it.tool.radius
    final = diameter / 2 - r
    if final < 0:
        raise CycleError("G87.2: Werkzeug ist größer als die Tasche")
    clockwise = _direction(ordered)
    w_start = it.plane.to_plane(it.position)[2]
    u0, v0, _safety, levels, _surface, fe, fz = _mill_levels(it, words, name)
    step = _stepover(it, words, r)
    radii = [final - k * step for k in range(math.ceil(final / step) - 1, -1, -1) if final - k * step > 1e-6]
    for level in levels:
        it.feed_to(_point(it, u0, v0, level), name, fz)
        for radius in radii:
            p = _point(it, u0 + radius, v0, level)
            it.feed_to(p, name, fe)
            it.arc_to(p, (u0, v0), clockwise, name, full_circle=True, feed=fe)
        it.feed_to(_point(it, u0, v0, level), name, fe)
    _finish(it, words, u0, v0, w_start, name)


def _boss(it: Interpreter, _variant: int, words: dict[str, Word], ordered: list[Word]) -> None:
    name = "G87.3 Zapfen"
    diameter = _get(it, words, "D")
    if diameter is None:
        raise CycleError("G87.3 ohne Zapfendurchmesser D")
    raw = _get(it, words, "DR")
    r = it.tool.radius
    if raw is None:
        raw = diameter + 4 * r
        it.note("G87.3 ohne Rohteildurchmesser DR: Zapfendurchmesser + 4 x Werkzeugradius angenommen")
    clockwise = _direction(ordered)
    final = diameter / 2 + r
    outer = raw / 2 + r
    step = _stepover(it, words, r)
    radii = [outer - k * step for k in range(0, max(1, math.ceil((outer - final) / step)))]
    radii = [x for x in radii if x > final + 1e-6] + [final]
    u0, v0, w_start = it.plane.to_plane(it.position)
    surface, depth, safety, _retract = _levels(it, words, w_start)
    levels = _infeeds(it, words, surface, depth)
    fe, fz = _get(it, words, "FE"), _get(it, words, "FZ")
    start_u = u0 + outer + r
    it.rapid_to(_point(it, u0, v0, safety), note=name)
    it.rapid_to(_point(it, start_u, v0, safety), note=name)
    for level in levels:
        it.feed_to(_point(it, start_u, v0, level), name, fz)
        for radius in radii:
            p = _point(it, u0 + radius, v0, level)
            it.feed_to(p, name, fe)
            it.arc_to(p, (u0, v0), clockwise, name, full_circle=True, feed=fe)
        it.feed_to(_point(it, start_u, v0, level), name, fe)
    _finish(it, words, u0, v0, w_start, name)


def _thread(it: Interpreter, variant: int, words: dict[str, Word], ordered: list[Word]) -> None:
    name = f"G87.{variant} {MILL_NAMES[variant]}"
    diameter, pitch = _get(it, words, "D"), _get(it, words, "ST")
    if diameter is None or pitch is None or pitch == 0:
        raise CycleError(f"G87.{variant} ohne Durchmesser D oder Steigung ST")
    r = it.tool.radius
    radius = diameter / 2 + r if variant == 4 else diameter / 2 - r
    if radius <= 0:
        raise CycleError(f"G87.{variant}: Werkzeug ist größer als das Gewinde")
    clockwise = _direction(ordered)
    u0, v0, w_start = it.plane.to_plane(it.position)
    surface, depth, safety, _retract = _levels(it, words, w_start)
    it.note("Gewindefräsen: Schraubenlinie vereinfacht (Anfahren im Viertelkreis nicht dargestellt)")
    it.rapid_to(_point(it, u0, v0, safety), note=name)
    if variant == 4:
        it.rapid_to(_point(it, u0 + radius + 2 * r, v0, safety), note=name)
        it.feed_to(_point(it, u0 + radius + 2 * r, v0, depth), name)
    else:
        it.feed_to(_point(it, u0, v0, depth), name)
    it.feed_to(_point(it, u0 + radius, v0, depth), name)
    turns = max((surface - depth) / abs(pitch), 0.0)
    steps = max(1, math.ceil(turns * 90))
    for i in range(1, steps + 1):
        t = i / steps
        angle = 2 * math.pi * turns * t * (-1 if clockwise else 1)
        it.feed_to(
            _point(it, u0 + radius * math.cos(angle), v0 + radius * math.sin(angle), depth + (surface - depth) * t),
            name,
        )
    _finish(it, words, u0, v0, w_start, name)


def _contour_pocket(it: Interpreter, _variant: int, words: dict[str, Word], _ordered: list[Word]) -> None:
    name = "G87.9 Konturtasche"
    u0, v0, w0 = it.plane.to_plane(it.position)
    _surface, depth, safety, retract = _levels(it, words, w0)
    it.rapid_to(_point(it, u0, v0, safety), note=name)
    it.contour_pocket = (depth, retract)
    it.note("G87.9 Konturtasche: Konturen werden auf Endtiefe gezeigt, das Ausräumen wird nicht berechnet")


# ----------------------------------------------------------------------- point patterns
def _execution(it: Interpreter, ordered: list[Word], call: SubprogramRef | None) -> str:
    if call is not None:
        return call.label
    for word in ordered:
        if word.address in ("P", "PP") and word.value:
            return f"{word.address}{word.value}"
    raise CycleError("Punktemuster ohne Ausführungsart (P.., PP.. oder %..)")


def _pattern(
    it: Interpreter, variant: int, words: dict[str, Word], ordered: list[Word], call: SubprogramRef | None
) -> None:
    execution = _execution(it, ordered, call)
    for ignored in ("SL", "SH"):
        if ignored in words:
            it.note("Schmierhub (SL/SH) wird im Simulator nicht dargestellt")
    for ignored in ("LA", "LB", "LC", "BA", "BB", "BC"):
        if ignored in words:
            it.note("Drehen/Schwenken von Rundachsen im Punktemuster wird nicht dargestellt")
    if variant == 1:
        points = _line_points(it, words)
        linear = True
        clockwise = False
        center = (0.0, 0.0)
    else:
        clockwise = _direction(ordered)
        points, center = _circle_points(it, words, clockwise)
        linear = any(w.address == "G" and w.value in ("1", "01") for w in ordered)
    if len(points) > MAX_PATTERN_POINTS:
        raise CycleError(f"Punktemuster mit mehr als {MAX_PATTERN_POINTS} Punkten")
    name = f"G89.{variant} {PATTERN_NAMES[variant]}"
    for index, (pu, pv) in enumerate(points):
        _u, _v, w = it.plane.to_plane(it.position)
        target = _point(it, pu, pv, w)
        if index == 0:
            if variant == 2:
                _rapid_linear(it, target, name)
        elif linear:
            _rapid_linear(it, target, name)
        else:
            it.arc_to(target, center, clockwise, name)
        it.execute_text(execution)


def _rapid_linear(it: Interpreter, target: Vec3, name: str) -> None:
    previous = it.rapid_variant
    it.rapid_variant = 2
    try:
        it.rapid_to(target, note=name)
    finally:
        it.rapid_variant = previous


def _line_points(it: Interpreter, words: dict[str, Word]) -> list[tuple[float, float]]:
    u0, v0, _ = it.plane.to_plane(it.position)
    count = int(_get(it, words, "LL") or 0)
    if count <= 0:
        raise CycleError("G89.1 ohne Punktezahl LL")
    lx, ly = _get(it, words, "LX", "LXI"), _get(it, words, "LY", "LYI")
    length, angle = _get(it, words, "L"), _get(it, words, "LW")
    if length is not None:
        step = (length * math.cos(math.radians(angle or 0.0)), length * math.sin(math.radians(angle or 0.0)))
    else:
        step = (lx or 0.0, ly or 0.0)
    rows = int(_get(it, words, "BL") or 1)
    bx, by = _get(it, words, "BX"), _get(it, words, "BY")
    row_len = _get(it, words, "B")
    if row_len is not None:
        bwa, bwi = _get(it, words, "BWA", "BW"), _get(it, words, "BWI")
        row_angle = bwa if bwa is not None else (math.degrees(math.atan2(step[1], step[0])) + (bwi or 90.0))
        row = (row_len * math.cos(math.radians(row_angle)), row_len * math.sin(math.radians(row_angle)))
    else:
        row = (bx or 0.0, by or 0.0)
    if rows > 1 and row == (0.0, 0.0):
        raise CycleError("G89.1: mehrere Reihen (BL) ohne Reihenabstand")
    return [(u0 + j * row[0] + k * step[0], v0 + j * row[1] + k * step[1]) for j in range(rows) for k in range(count)]


def _circle_points(
    it: Interpreter, words: dict[str, Word], clockwise: bool
) -> tuple[list[tuple[float, float]], tuple[float, float]]:
    u0, v0, _ = it.plane.to_plane(it.position)
    cu = _get(it, words, "LX", "LXA")
    cv = _get(it, words, "LY", "LYA")
    cui, cvi = _get(it, words, "LXI"), _get(it, words, "LYI")
    center = (
        cu if cu is not None else u0 + (cui or 0.0),
        cv if cv is not None else v0 + (cvi or 0.0),
    )
    radius = _get(it, words, "R")
    start = _get(it, words, "AW")
    if radius is None or start is None:
        raise CycleError("G89.2 ohne Radius R oder Startwinkel AW")
    sign = -1.0 if clockwise else 1.0
    count = _get(it, words, "L")
    pitch = _get(it, words, "TW")
    ewa, ewi = _get(it, words, "EWA", "EW"), _get(it, words, "EWI")
    if pitch is not None:
        if count is None:
            span = ewi if ewi is not None else ((ewa - start) if ewa is not None else 360.0 - abs(pitch))
            count = math.floor(abs(span) / abs(pitch) + 1e-9) + 1
        angles = [start + sign * abs(pitch) * k for k in range(int(count))]
    elif count is not None and count >= 1:
        n = int(count)
        if ewa is not None or ewi is not None:
            span = ewi if ewi is not None else (ewa - start)  # type: ignore[operator]
            angles = [start + span * k / max(n - 1, 1) for k in range(n)]
        else:
            angles = [start + sign * 360.0 * k / n for k in range(n)]
    else:
        raise CycleError("G89.2: Punkteaufteilung unvollständig (TW, EW oder L fehlt)")
    points = [
        (center[0] + radius * math.cos(math.radians(a)), center[1] + radius * math.sin(math.radians(a))) for a in angles
    ]
    return points, center
