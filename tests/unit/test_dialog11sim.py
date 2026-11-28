"""DIALOG 11 offline simulator: syntax, parameters, motion, compensation, cycles, playback."""

from __future__ import annotations

import math

import pytest

from deckellink.dialog11sim.expressions import ExpressionError, evaluate, is_arithmetic
from deckellink.dialog11sim.geometry import PLANES, Transform, arc_points, circle_center_from_radius, offset_polyline
from deckellink.dialog11sim.interpreter import simulate
from deckellink.dialog11sim.model import MoveKind, Severity, SimConfig, SimResult, ToolData
from deckellink.dialog11sim.playback import Playback, format_duration
from deckellink.dialog11sim.reference import DEMO_PARAMETERS, DEMO_PROGRAM, FUNCTION_REFERENCE
from deckellink.dialog11sim.syntax import SubprogramRef, parse_line, split_programs, tokenize_words

XY = PLANES[17]


def run(text: str, radius: float = 5.0, **kwargs: object) -> SimResult:
    config = SimConfig(tools={1: ToolData(radius=radius)}, **kwargs)  # type: ignore[arg-type]
    return simulate(text, config)


def end_of(result: SimResult) -> tuple[float, float, float]:
    p = result.moves[-1].end
    return round(p[0], 3), round(p[1], 3), round(p[2], 3)


def codes(result: SimResult, severity: Severity) -> list[str]:
    return [m.text for m in result.messages if m.severity is severity]


# ---- syntax -----------------------------------------------------------------------------
def test_tokenize_words_and_parameters() -> None:
    words, call = tokenize_words("G1XI-40 YA10 XP1 Z-P3 TIPP2 F200")
    assert [str(w) for w in words] == ["G1", "XI-40", "YA10", "XP1", "Z-P3", "TIPP2", "F200"]
    assert call is None
    assert words[3].is_parameter and not words[0].is_parameter
    words, call = tokenize_words("L3 %*1")
    assert call == SubprogramRef("sub", 1) and [str(w) for w in words] == ["L3"]
    assert tokenize_words("%%4")[1] == SubprogramRef("macro", 4)
    assert tokenize_words("%%*2")[1] == SubprogramRef("macro_sub", 2)
    assert SubprogramRef("macro_sub", 2).label == "%%*2"


@pytest.mark.parametrize("text", ["G87.2 D20", "G87*2 D20", "G87 + 2 D20", "G87 • 2 D20"])
def test_g_variants(text: str) -> None:
    block = parse_line(1, f"N10 {text}")
    words = block.statements[0].words
    assert (words[0].address, words[0].value, words[0].variant) == ("G", "87", 2)
    assert str(words[0]) == "G87.2"


def test_parse_line_statements() -> None:
    block = parse_line(3, "N20 G17 T1 M6; M3 (KOMMENTAR)")
    assert block.number == 20 and len(block.statements) == 2 and not block.problem
    assign = parse_line(4, "N30 P1 = (SIN (30) x 10)").statements[0]
    assert (assign.kind, assign.target, assign.expression) == ("assign", "P1", "(SIN (30) X 10)")
    calc = parse_line(5, "N40 PP4!").statements[0]
    assert (calc.kind, calc.target) == ("calc", "PP4")
    cond = parse_line(6, "N50 IF P1 >= 3 THEN L0 N90 ELSE G0 Z100").statements[0]
    assert (cond.kind, cond.expression, cond.then_text, cond.else_text) == ("if", "P1 >= 3", "L0 N90", "G0 Z100")
    assert parse_line(7, "N60 G1 X10 ?").problem


def test_split_programs() -> None:
    main, others = split_programs("%1\nN10 G0 X0\nN20 IF P1 > 1 THEN L0 N10\nELSE G0 Z1\n%*1\nN10 G1 X5\n%%4\nN1 M17\n")
    assert len(main.blocks) == 2 and main.blocks[1].statements[0].else_text == "G0 Z1"
    assert set(others) == {SubprogramRef("sub", 1), SubprogramRef("macro", 4)}
    assert main.index_of(20) == 1 and main.index_of(99) is None
    assert main.label == "Hauptprogramm"


# ---- expressions ----------------------------------------------------------------------------
def test_expressions() -> None:
    values = {"P1": 10.0, "PP2": 4.0}

    def resolve(name: str) -> float:
        return values[name]

    assert evaluate("(P1 + PP2 x 2)", resolve) == 18
    assert evaluate("(P1 / 4 : 2)", resolve) == 1.25
    assert evaluate("-P1 + 3", resolve) == -7
    assert evaluate("SIN (30) x 2", resolve) == pytest.approx(1.0)
    assert evaluate("SQRT (16) + SQR (3) + ABS (-2) + INT (2.7) + FRC (2.25)", resolve) == pytest.approx(17.25)
    assert evaluate("RND (2.5) + NEG (1) + PI", resolve) == pytest.approx(2 + math.pi)
    assert evaluate("ATAN (1) + ACOS (1) + ASIN (0) + COS (0) + TAN (45)", resolve) == pytest.approx(47.0)
    for bad in ("(1 + 2", "1 / 0", "ASIN (2)", "1 2", "1 & 2", ""):
        with pytest.raises(ExpressionError):
            evaluate(bad, resolve)
    assert is_arithmetic("(P1+2)") and is_arithmetic("-P3") and is_arithmetic("12.5")
    assert not is_arithmetic("G81 OA0") and not is_arithmetic("%*1")


# ---- motion ---------------------------------------------------------------------------------
def test_linear_absolute_incremental_and_end() -> None:
    result = run("N10 G0 X0 Y0 Z10\nN20 G1 X10 Y5 F100\nN30 XI5 YI-5\nN40 G91 X1\nN50 XA0\nN60 M30\n")
    assert result.end_reason.startswith("M30")
    feeds = [m for m in result.moves if m.kind is MoveKind.FEED]
    assert [tuple(round(c, 3) for c in m.end) for m in feeds] == [(10, 5, 10), (15, 0, 10), (16, 0, 10), (0, 0, 10)]
    assert feeds[0].duration == pytest.approx(math.hypot(10, 5) / 100 * 60)
    assert not result.errors


def test_rapid_variants() -> None:
    dog = run("N10 G0 X10 Y4 Z0\nM30", start_position=(0.0, 0.0, 0.0))
    assert [tuple(round(c, 3) for c in p) for p in dog.moves[0].points] == [(0, 0, 0), (4, 4, 0), (10, 4, 0)]
    down = run("N10 G0.1 X10 Y10 Z-5\nM30", start_position=(0.0, 0.0, 10.0))
    assert down.moves[0].points[1] == (10.0, 10.0, 10.0)  # plane first, then infeed
    up = run("N10 G0*1 X10 Y10 Z50\nM30", start_position=(0.0, 0.0, 10.0))
    assert up.moves[0].points[1] == (0.0, 0.0, 50.0)  # retract first
    linear = run("N10 G0+2 X10 Y4 Z0\nM30", start_position=(0.0, 0.0, 0.0))
    assert len(linear.moves[0].points) == 2


def test_arcs_full_circle_polar_and_radius() -> None:
    result = run("N10 G0 X10 Y0 Z0\nN20 G3 X0 Y10 I0 J0 F100\nN30 G2 I0 J0\nM30", start_position=(0.0, 0.0, 0.0))
    quarter, full = result.moves[1], result.moves[2]
    assert quarter.length == pytest.approx(math.pi * 10 / 2, rel=1e-3)
    assert full.length == pytest.approx(2 * math.pi * 10, rel=1e-3)
    assert all(math.hypot(p[0], p[1]) == pytest.approx(10) for p in quarter.points)
    polar = run("N10 G0 X0 Y0 Z0\nN20 G1 PX50 PY40 PW-135 PL20 F100\nM30", start_position=(0.0, 0.0, 0.0))
    x, y, _ = polar.moves[-1].end
    assert (x, y) == pytest.approx((50 - 20 / math.sqrt(2), 40 - 20 / math.sqrt(2)))
    g9 = run("N10 G0 X0 Y0 Z0\nN20 G1 G9 I50 J40 W-135 R20 F100\nM30", start_position=(0.0, 0.0, 0.0))
    assert g9.moves[-1].end[:2] == pytest.approx(polar.moves[-1].end[:2])
    arc_g9 = run("N10 G0 X10 Y0 Z0\nN20 G3 G9 I0 J0 WI90 F100\nM30", start_position=(0.0, 0.0, 0.0))
    assert arc_g9.moves[-1].end[:2] == pytest.approx((0.0, 10.0), abs=1e-6)
    radius = run("N10 G0 X0 Y0 Z0\nN20 G2 X20 Y0 R10 F100\nM30", start_position=(0.0, 0.0, 0.0))
    assert radius.moves[-1].length == pytest.approx(math.pi * 10, rel=1e-3)
    assert any("Radius R" in t for t in codes(radius, Severity.INFO))
    bad = run("N10 G2 X20 Y0 F100\nN20 G2 X100 Y0 R10\nN30 G3 G9 WA10\nM30")
    assert len(bad.errors) == 3


def test_helix_and_mismatched_arc_warning() -> None:
    result = run(
        "N10 G0 X10 Y0 Z0\nN20 G3 X10 Y0 Z-5 I0 J0 F100\nN30 G3 X0 Y12 I0 J0\nM30", start_position=(0.0, 0.0, 0.0)
    )
    assert result.moves[1].end[2] == pytest.approx(-5)
    assert any("Zielradius" in t for t in codes(result, Severity.WARNING))


def test_coordinate_transformations() -> None:
    shifted = run("N10 G56 X100 Y50\nN20 G0 X0 Y0 Z0\nN30 G55 X10\nN40 G0 X0 Y0\nN50 G53\nN60 G0 X0 Y0\nM30")
    ends = [tuple(round(c, 3) for c in m.end) for m in shifted.moves]
    assert ends[0] == (100, 50, 0) and ends[1] == (110, 50, 0) and ends[2] == (0, 0, 0)
    rotated = run("N10 G55 W90\nN20 G0 X10 Y0 Z0\nM30")
    assert end_of(rotated) == (0, 10, 0)
    mirrored = run("N10 G58 X0\nN20 G0 X10 Y5 Z0\nN30 G58\nN40 G0 X10 Y5\nM30")
    assert tuple(round(c, 3) for c in mirrored.moves[0].end) == (-10, 5, 0)
    assert end_of(mirrored) == (10, 5, 0)
    scaled = run("N10 G59 X2 Y0.5\nN20 G0 X10 Y10 Z0\nM30")
    assert end_of(scaled) == (20, 5, 0)
    preset = run("N10 G0 X30 Y0 Z0\nN20 G54 X0\nN30 G0 X10\nM30")
    assert end_of(preset) == (40, 0, 0)
    table = run("N10 G51.3\nN20 G0 X0 Y0 Z0\nN30 G51.7\nM30", zero_points={3: (5.0, 6.0, 7.0)})
    assert tuple(round(c, 3) for c in table.moves[0].end) == (5, 6, 7)
    assert any("G51.7" in t for t in codes(table, Severity.INFO))


def test_transform_roundtrip_and_reverse() -> None:
    t = Transform((5.0, 1.0, 0.0), 30.0, XY, (2.0, None, None), (1.5, -1.0, 1.0))
    p = (3.0, 4.0, 5.0)
    assert t.invert(t.apply(p)) == pytest.approx(p)
    assert t.reverses_direction is False  # mirror X and negative scale Y -> two flips
    assert Transform(mirror=(1.0, None, None)).reverses_direction


# ---- parameters, flow ---------------------------------------------------------------------------
def test_parameters_loops_and_jumps() -> None:
    text = (
        "N10 P1 = 0\nN20 P2 = (P1 + PT5)\nN30 P1 = (P1 + 1)\nN40 IF P1 < 4 THEN L0 N30 ELSE P3 = 99\n"
        "N50 G0 XP2 YP3 Z-P1\nN60 PP7!\nN70 M30\n"
    )
    result = run(text, parameter_table={"P5": "2.5", "PP7": "(PP8 x 2)", "PP8": "21"})
    assert result.parameters["P1"] == "4" and result.parameters["P3"] == "99" and result.parameters["PP7"] == "42"
    assert end_of(result) == (2.5, 99, -4)


def test_parameter_errors_and_endless_loop() -> None:
    result = run("N10 G0 XP9\nN20 P1!\nN30 P2 = G81 OA0\nN40 G0 XP2\nM30")
    assert len(result.errors) == 3
    endless = run("N10 L0 N10\n", max_blocks=500)
    assert "Endlosschleife" in endless.end_reason and endless.errors
    missing = run("N10 L0 N99\nM30")
    assert "N99" in missing.end_reason


def test_subprograms_repeats_and_parameter_blocks() -> None:
    text = (
        "N10 G0 X0 Y0 Z0\nN20 L3 %*1\nN30 G1 X100 F100\nN40 X110\nN50 L2 N30 N40\nN60 P4 = G1 Y7\nN70 P4\n"
        "N80 %%4\nN90 %*9\nN100 M30\n%*1\nN1 G91 G1 X1 F100\nN2 G90\nN3 M17\nN4 G1 X999\n%%4\nN1 G1 Z-3\n"
    )
    result = run(text, start_position=(0.0, 0.0, 0.0))
    xs = [round(m.end[0], 3) for m in result.moves]
    assert xs[:3] == [1, 2, 3] and 999 not in xs
    assert xs.count(100) == 3 and sum(1 for m in result.moves if m.line == 4) == 3
    assert end_of(result) == (110, 7, -3)
    assert any("%*9" in t for t in codes(result, Severity.ERROR))


def test_m17_in_main_and_program_without_end() -> None:
    result = run("N10 G0 X1 Y1 Z1\nN20 M17\n")
    assert any("M17" in t for t in codes(result, Severity.WARNING))
    assert any("ohne M30" in t for t in codes(result, Severity.WARNING))


def test_unknown_functions_are_reported_not_executed() -> None:
    result = run("N10 G0 X0 Y0 Z0 C90\nN20 G70\nN30 M8\nN40 G61 R5\nN50 G1 X10 UR5 F100\nN60 T7\nM30")
    infos = " ".join(codes(result, Severity.INFO))
    for expected in ("C-Achse", "G70", "M8", "G61", "Konturzug", "T7"):
        assert expected in infos
    assert result.snapshots[-1].c_axis == 90


# ---- radius compensation -----------------------------------------------------------------------
SQUARE = "N5 T1\nN10 G0 X-30 Y-30 Z0\nN20 G42 G45 R5 X-15 Y-15\nN30 G1 X15 F100; Y15; X-15; Y-15\nN40 G40 G45 R5\nM30"


def test_radius_compensation_outside_square() -> None:
    result = run(SQUARE, radius=4, start_position=(0.0, 0.0, 0.0))
    comp = [m for m in result.moves if m.compensated]
    assert comp[0].note == "Anfahren" and comp[1].note == "Anfahren G45"
    assert comp[1].end[:2] == pytest.approx((-15, -19))
    contour = comp[2:-1]
    xs = [p[0] for m in contour for p in m.points]
    ys = [p[1] for m in contour for p in m.points]
    assert min(xs) == pytest.approx(-19) and max(xs) == pytest.approx(19) and max(ys) == pytest.approx(19)
    assert comp[-1].note == "Wegfahren G45"
    assert len(result.programmed) > len(comp) - 3
    inside = run(SQUARE.replace("G42", "G41"), radius=4, start_position=(0.0, 0.0, 0.0))
    inner = [p for m in inside.moves if m.compensated and not m.note for p in m.points]
    assert max(p[0] for p in inner) == pytest.approx(11) and max(p[1] for p in inner) == pytest.approx(11)
    corners = {(round(p[0], 3), round(p[1], 3)) for p in inner}
    assert {(11, -11), (11, 11), (-11, 11)} <= corners  # inner corners = intersections
    assert (-15, -11) in corners and (-11, -15) in corners  # open contour: ends are not joined


def test_compensation_quarter_and_semicircle_approach() -> None:
    for kind in (46, 47):
        result = run(SQUARE.replace("G45", f"G{kind}"), radius=4, start_position=(0.0, 0.0, 0.0))
        approach = next(m for m in result.moves if m.note == f"Anfahren G{kind}")
        assert approach.end[:2] == pytest.approx((-15, -19))
        assert len(approach.points) > 3
        assert any(m.note == f"Wegfahren G{kind}" for m in result.moves)


def test_compensation_warnings() -> None:
    result = run("N10 T0\nN20 G41 X0 Y0\nN30 G1 X10 F100\nN40 G18\nM30")
    warnings = " ".join(codes(result, Severity.WARNING))
    assert "ohne Werkzeugradius" in warnings and "Ebenenwechsel" in warnings
    unfinished = run("N10 G41 X0 Y0\nN20 G1 X10 F100\n")
    assert any("nicht mit G40" in t for t in codes(unfinished, Severity.WARNING))


def test_offset_polyline_corners() -> None:
    square = [(0.0, 0.0, 0.0), (10.0, 0.0, 0.0), (10.0, 10.0, 0.0), (0.0, 10.0, 0.0)]
    inner, src = offset_polyline(square, [0, 1, 2, 3], XY, 1.0, left=True)
    assert inner[1][:2] == pytest.approx((9.0, 1.0))  # inner corner = intersection
    assert len(inner) == len(src)
    outer, _ = offset_polyline(square, [0, 1, 2, 3], XY, 1.0, left=False)
    assert len(outer) > 4 and all(math.dist(p[:2], (10.0, 0.0)) == pytest.approx(1.0) for p in outer[1:-1][:5])
    back, _ = offset_polyline([(0.0, 0.0, 0.0), (10.0, 0.0, 0.0), (0.0, 0.0001, 0.0)], [0, 1, 2], XY, 1.0, True)
    assert len(back) >= 3
    single, _ = offset_polyline([(0.0, 0.0, 0.0)], [0], XY, 1.0, True)
    assert single == [(0.0, 0.0, 0.0)]


def test_geometry_helpers() -> None:
    assert circle_center_from_radius((0, 0), (20, 0), 10, True) == pytest.approx((10, 0))
    short_cw = circle_center_from_radius((0, 0), (10, 10), 10, True)
    long_cw = circle_center_from_radius((0, 0), (10, 10), -10, True)
    assert short_cw is not None and long_cw is not None and short_cw != long_cw
    assert circle_center_from_radius((0, 0), (30, 0), 10, True) is None
    pts = arc_points(XY, (10.0, 0.0, 0.0), (10.0, 0.0, 0.0), (0.0, 0.0), clockwise=True)
    assert pts[-1] == (10.0, 0.0, 0.0) and len(pts) >= 90
    assert PLANES[18].label.startswith("G18") and PLANES[19].from_plane(1, 2, 3) == (3, 1, 2)


# ---- cycles ------------------------------------------------------------------------------------
def test_drilling_cycles() -> None:
    result = run("N10 G0 X10 Y10 Z20\nN20 G81 OA0 TI10 SI2 EA5 G4 F2 F80\nM30")
    z = [round(m.end[2], 3) for m in result.moves[1:]]
    assert z == [2, -10, 2, 5]
    assert result.moves[2].feed == 80 and result.dwells[0].seconds == 2
    peck = run("N10 G0 X0 Y0 Z10\nN20 G83 OA0 TA-20 SI2 EA10 MI8 DI2 F100\nM30")
    depths = [round(m.end[2], 3) for m in peck.moves if m.kind is MoveKind.FEED]
    assert depths == [-8, -14, -18, -20]
    chip = run("N10 G0 X0 Y0 Z10\nN20 G82 OI10 TI6 MI2 F100\nM30")
    assert [round(m.end[2], 3) for m in chip.moves if m.kind is MoveKind.FEED] == [-2, -4, -6]
    assert any("Rückzugsebene" in t for t in codes(chip, Severity.WARNING))
    tap = run("N10 S500\nN20 G0 X0 Y0 Z10\nN30 G84 OA0 TI10 SI2 EA5 ST1.5\nM30")
    assert [m.feed for m in tap.moves if m.kind is MoveKind.FEED] == [750, 750]
    for number in (85, 86):
        other = run(f"N10 G0 X0 Y0 Z10\nN20 G{number} OA0 TI10 SI2 EA5 F50\nM30")
        assert not other.errors
    missing = run("N10 G81 OA0 SI2\nM30")
    assert any("Tiefe" in t for t in codes(missing, Severity.ERROR))


def test_pocket_boss_thread_cycles() -> None:
    rect = run("N10 T1\nN20 G0 X0 Y0 Z10\nN30 G87.1 OA0 TI6 SI2 EA5 LI40 BI20 R6 MI3 AM4 G2 FE200 DW30\nM30", radius=3)
    pts = [p for m in rect.moves if m.kind is MoveKind.FEED for p in m.points]
    assert min(p[2] for p in pts) == pytest.approx(-6)
    assert max(math.hypot(p[0], p[1]) for p in pts) <= math.hypot(17, 7) + 1e-6
    assert end_of(rect) == (0, 0, 5)
    circle = run("N10 T1\nN20 G0 X0 Y0 Z10\nN30 G87.2 OA0 TI4 SI2 EA5 D30 AM5\nM30", radius=3)
    assert max(math.hypot(p[0], p[1]) for m in circle.moves for p in m.points) == pytest.approx(12)
    boss = run("N10 T1\nN20 G0 X0 Y0 Z10\nN30 G87.3 OA0 TI4 SI2 EA5 D20 DR40 AM5\nM30", radius=3)
    radii = [math.hypot(p[0], p[1]) for m in boss.moves if m.kind is MoveKind.FEED for p in m.points]
    assert min(r for r in radii if r > 1) == pytest.approx(13)
    for variant in (4, 5):
        thread = run(f"N10 T1\nN20 G0 X0 Y0 Z10\nN30 G87.{variant} OA0 TI6 SI2 EA5 D20 ST2 G2\nM30", radius=3)
        assert not thread.errors and thread.feed_length > 100
    errors = run(
        "N10 T1\nN20 G87.1 OA0 TI5\nN30 G87.1 OA0 TI5 LI4 BI4\nN40 G87.2 OA0 TI5\nN50 G87.2 OA0 TI5 D2\n"
        "N60 G87.3 OA0 TI5\nN70 G87.4 OA0 TI5 D10\nN80 G87.5 OA0 TI5 D2 ST1\nN90 G87.7 OA0 TI5\nM30",
        radius=3,
    )
    assert len(errors.errors) == 8
    defaults = run("N10 T1\nN20 G87.3 OA0 TI5 D20 FA2\nN30 G87.1 OA0 TI5 LI30 BI30 FA2\nM30", radius=3)
    assert any("DR" in t for t in codes(defaults, Severity.INFO))
    assert any("AM" in t for t in codes(defaults, Severity.INFO))


def test_contour_pocket() -> None:
    text = (
        "N10 T1\nN20 G0 X0 Y0 Z10\nN30 G87.9 OA0 SI2 TA-8 EA5\nN40 G41 X5 Y5 G0\nN50 G1 X20 F100\nN60 Y20\n"
        "N70 G40\nN80 M79\nN90 M79\nM30"
    )
    result = run(text, radius=2)
    feed_z = {round(p[2], 3) for m in result.moves if m.kind is MoveKind.FEED for p in m.points[1:]}
    assert feed_z == {-8}
    assert end_of(result)[2] == 5
    assert any("M79 ohne" in t for t in codes(result, Severity.WARNING))


def test_point_patterns() -> None:
    line = run("N10 G0 X0 Y0 Z10\nN20 G89.1 LL3 LX10 BL2 BY5 P1\nM30", parameter_table={"P1": "G81 OA0 TI3 SI2 EA5"})
    holes = {(round(m.end[0], 3), round(m.end[1], 3)) for m in line.moves if m.note.startswith("G81")}
    assert holes == {(0, 0), (10, 0), (20, 0), (0, 5), (10, 5), (20, 5)}
    polar = run("N10 G0 X0 Y0 Z10\nN20 G89.1 LL2 L10 LW90 B4 BWA0 BL2 %*1\nM30\n%*1\nN1 G81 OA0 TI3 SI2 EA5\n")
    polar_holes = {(round(m.end[0], 3), round(m.end[1], 3)) for m in polar.moves if m.note.startswith("G81")}
    assert polar_holes == {(0, 0), (0, 10), (4, 0), (4, 10)}
    circle = run(
        "N10 G0 X0 Y0 Z10\nN20 G89.2 LX0 LY0 R20 AW0 L4 G1 PP1 SL5 SH100 LA5\nN30 G89.2 LX0 LY0 R20 AW0 TW90 "
        "EWA180 PP1\nN40 G89.2 LXI0 LYI0 R20 AW0 L3 EWI90 PP1\nM30",
        parameter_table={"PP1": "G81 OA0 TI3 SI2 EA5"},
    )
    circle_holes = [
        (round(m.end[0], 3), round(m.end[1], 3)) for m in circle.moves if m.note == "G81 Bohren" and m.end[2] < 0
    ]
    assert circle_holes[:4] == [(20, 0), (0, 20), (-20, 0), (0, -20)]
    assert circle_holes[4:7] == [(20, 0), (0, 20), (-20, 0)]
    assert len(circle_holes) == 10
    infos = " ".join(codes(circle, Severity.INFO))
    assert "Schmierhub" in infos and "Rundachsen" in infos
    bad = run(
        "N10 G89.1 LX10 P1\nN20 G89.1 LL2 BL2 P1\nN30 G89.2 R10 P1\nN40 G89.2 R10 AW0 P1\nN50 G89.1 LL2\n"
        "N60 G89.3 P1\nM30"
    )
    assert len(bad.errors) == 6


# ---- playback ------------------------------------------------------------------------------------
def test_playback_frames_and_blocks() -> None:
    result = run("N10 G0 X0 Y0 Z0\nN20 G1 X60 F60\nN30 G4 F5\nN40 G1 Y60\nM30", start_position=(0.0, 0.0, 0.0))
    pb = Playback(result, (0.0, 0.0, 0.0))
    assert pb.total == pytest.approx(60 + 5 + 60)
    mid = pb.frame(30)
    assert mid.position[0] == pytest.approx(30) and mid.line == 2 and mid.remaining[0] == pytest.approx(30)
    assert pb.frame(62).position == pytest.approx((60, 0, 0))
    end = pb.frame(1e9)
    assert end.finished and end.position == pytest.approx((60, 60, 0))
    assert pb.next_block_end(0) == pytest.approx(60)
    assert pb.next_block_end(60) == pytest.approx(125)
    assert pb.next_block_end(1000) == pb.total
    assert pb.time_of_line(4) == pytest.approx(65) and pb.time_of_line(99) is None
    assert pb.frame(-5).move_index == 0
    empty = Playback(SimResult(), (1.0, 2.0, 3.0))
    assert empty.frame(3).position == (1.0, 2.0, 3.0) and empty.next_block_end(0) == 0
    assert format_duration(3725.4) == "1:02:05" and format_duration(-3) == "0:00:00"


def test_demo_program_runs_cleanly() -> None:
    result = simulate(DEMO_PROGRAM, SimConfig(tools={1: ToolData(radius=4)}, parameter_table=dict(DEMO_PARAMETERS)))
    assert result.end_reason.startswith("M30") and not result.errors and not result.warnings
    assert result.bounds() is not None and result.total_time_s > 0
    assert len(FUNCTION_REFERENCE) > 30


def test_self_referencing_parameter_is_an_error_not_a_crash() -> None:
    config = SimConfig(parameter_table={"P8": "(P8 + 1)"})
    result = simulate("N10 G1 XP8 F100\nN20 M30\n", config)
    assert [m.text for m in result.errors] == ["Parameter-Verweise zu tief verschachtelt"]
    assert result.end_reason.startswith("M30")


def test_point_limit_during_final_compensation_flush_ends_cleanly() -> None:
    config = SimConfig(max_points=6, tools={1: ToolData(radius=2.0)})
    text = "N10 T1\nN20 G41 X10 Y0\nN30 G1 X20 F100\nN40 X30\nN50 X40\n"
    result = simulate(text, config)
    assert result.end_reason == "Abbruch: zu viele Bahnpunkte"
